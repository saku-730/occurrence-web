use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header::COOKIE},
};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    features::auth::service::{AuthService, AuthServiceError},
    state::AppState,
};

use super::{gbif::GbifClient, repository::PaperRepository};

const GBIF_LOOKUP_CONCURRENCY: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePaperTaxaRequest {
    pub scientific_names: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPaperTaxon {
    pub scientific_name: String,
    pub to_taxon: Option<String>,
    pub taxon_scientific_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePaperTaxaResponse {
    pub matches: Vec<ResolvedPaperTaxon>,
}

#[derive(Debug)]
pub enum ResolvePaperTaxaError {
    InvalidSession,
    InvalidInput,
    NotFound,
    Database(sqlx::Error),
}

impl From<AuthServiceError> for ResolvePaperTaxaError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<sqlx::Error> for ResolvePaperTaxaError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl axum::response::IntoResponse for ResolvePaperTaxaError {
    fn into_response(self) -> axum::response::Response {
        use axum::{Json, http::StatusCode};
        use crate::features::auth::dto::ErrorResponse;

        let (status, error, message) = match self {
            Self::InvalidSession => (StatusCode::UNAUTHORIZED, "invalid_session", "Invalid session"),
            Self::InvalidInput => (StatusCode::BAD_REQUEST, "invalid_taxon_request", "Invalid taxon resolution request"),
            Self::NotFound => (StatusCode::NOT_FOUND, "paper_not_found", "Paper not found"),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error", "Internal server error"),
        };

        (status, Json(ErrorResponse {
            error: error.to_string(),
            message: message.to_string(),
        }))
            .into_response()
    }
}

pub async fn resolve_taxa(
    State(state): State<AppState>,
    Path(paper_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<ResolvePaperTaxaRequest>,
) -> Result<Json<ResolvePaperTaxaResponse>, ResolvePaperTaxaError> {
    if request.scientific_names.is_empty() || request.scientific_names.len() > 1000 {
        return Err(ResolvePaperTaxaError::InvalidInput);
    }

    let session_token = extract_session_token(&headers)?;
    let _current_user = AuthService::current_user(&state.posgre, session_token).await?;
    if PaperRepository::find_by_id(&state.posgre, paper_id).await?.is_none() {
        return Err(ResolvePaperTaxaError::NotFound);
    }

    let normalized = request
        .scientific_names
        .into_iter()
        .map(|value| value.trim().to_string())
        .collect::<Vec<_>>();

    if normalized.iter().any(String::is_empty) {
        return Err(ResolvePaperTaxaError::InvalidInput);
    }

    let mut unique_names = normalized.clone();
    unique_names.sort();
    unique_names.dedup();

    let client = GbifClient::new().ok();
    let resolved = stream::iter(unique_names.into_iter().map(|scientific_name| {
        let client = client.clone();
        async move {
            let matched = match client {
                Some(client) => client.match_taxon(&scientific_name).await.ok().flatten(),
                None => None,
            };
            (scientific_name, matched)
        }
    }))
    .buffered(GBIF_LOOKUP_CONCURRENCY)
    .collect::<HashMap<_, _>>()
    .await;

    let matches = normalized
        .into_iter()
        .map(|scientific_name| {
            let matched = resolved.get(&scientific_name).cloned().flatten();
            ResolvedPaperTaxon {
                to_taxon: matched.as_ref().map(|value| value.taxon_uri.clone()),
                taxon_scientific_name: matched.map(|value| value.scientific_name),
                scientific_name,
            }
        })
        .collect();

    Ok(Json(ResolvePaperTaxaResponse { matches }))
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, ResolvePaperTaxaError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(ResolvePaperTaxaError::InvalidSession)?
        .to_str()
        .map_err(|_| ResolvePaperTaxaError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            if !token.trim().is_empty() {
                return Ok(token.to_string());
            }
        }
    }

    Err(ResolvePaperTaxaError::InvalidSession)
}
