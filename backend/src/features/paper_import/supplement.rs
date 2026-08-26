use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    features::auth::{
        dto::ErrorResponse,
        service::{AuthService, AuthServiceError},
    },
    state::AppState,
};

use super::repository::PaperRepository;

#[derive(Debug, Deserialize, ToSchema)]
pub struct SupplementBibliographicMetadataRequest {
    pub doi: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SupplementBibliographicMetadataResponse {
    pub paper_id: Uuid,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub requires_bibliographic_input: bool,
    pub message: String,
}

#[derive(Debug)]
pub enum SupplementBibliographicMetadataError {
    InvalidSession,
    InvalidInput,
    NotFound,
    Database(sqlx::Error),
}

impl From<AuthServiceError> for SupplementBibliographicMetadataError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl IntoResponse for SupplementBibliographicMetadataError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidSession => (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_session".to_string(),
                    message: "Invalid session".to_string(),
                }),
            )
                .into_response(),
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_bibliographic_metadata".to_string(),
                    message: "A non-empty DOI or title is required".to_string(),
                }),
            )
                .into_response(),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "paper_not_found".to_string(),
                    message: "Paper not found".to_string(),
                }),
            )
                .into_response(),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                }),
            )
                .into_response(),
        }
    }
}

#[utoipa::path(
    patch,
    path = "/papers/{paper_id}/bibliographic-metadata",
    params(
        ("paper_id" = Uuid, Path, description = "Paper UUID returned by /paper-import")
    ),
    request_body = SupplementBibliographicMetadataRequest,
    responses(
        (
            status = 200,
            description = "Missing DOI/title metadata filled without overwriting existing GROBID values",
            body = SupplementBibliographicMetadataResponse
        ),
        (
            status = 400,
            description = "Neither a non-empty DOI nor title was supplied",
            body = ErrorResponse
        ),
        (
            status = 401,
            description = "Authentication required",
            body = ErrorResponse
        ),
        (
            status = 404,
            description = "Paper does not exist",
            body = ErrorResponse
        ),
        (
            status = 500,
            description = "PostgreSQL operation failed",
            body = ErrorResponse
        )
    ),
    tag = "paper-import"
)]
pub async fn supplement_bibliographic_metadata(
    State(state): State<AppState>,
    Path(paper_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<SupplementBibliographicMetadataRequest>,
) -> Result<Json<SupplementBibliographicMetadataResponse>, SupplementBibliographicMetadataError> {
    let session_token = extract_session_token(&headers)?;
    let _current_user = AuthService::current_user(&state.posgre, session_token).await?;

    let doi = normalize_doi(request.doi);
    let title = normalize_text(request.title);

    if doi.is_none() && title.is_none() {
        return Err(SupplementBibliographicMetadataError::InvalidInput);
    }

    let paper = PaperRepository::fill_missing_bibliographic_identity(
        &state.posgre,
        paper_id,
        doi.as_deref(),
        title.as_deref(),
    )
    .await
    .map_err(SupplementBibliographicMetadataError::Database)?
    .ok_or(SupplementBibliographicMetadataError::NotFound)?;

    let requires_bibliographic_input = paper.doi.is_none() && paper.title.is_none();

    Ok(Json(SupplementBibliographicMetadataResponse {
        paper_id: paper.id,
        doi: paper.doi,
        title: paper.title,
        requires_bibliographic_input,
        message: if requires_bibliographic_input {
            "bibliographic metadata still requires a DOI or title"
        } else {
            "bibliographic metadata completed"
        }
        .to_string(),
    }))
}

fn normalize_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_doi(value: Option<String>) -> Option<String> {
    let value = normalize_text(value)?;
    let trimmed = value
        .strip_prefix("https://doi.org/")
        .or_else(|| value.strip_prefix("http://doi.org/"))
        .or_else(|| value.strip_prefix("http://dx.doi.org/"))
        .or_else(|| value.strip_prefix("https://dx.doi.org/"))
        .or_else(|| value.strip_prefix("doi:"))
        .unwrap_or(&value)
        .trim()
        .to_string();

    (!trimmed.is_empty()).then_some(trimmed)
}

fn extract_session_token(
    headers: &HeaderMap,
) -> Result<String, SupplementBibliographicMetadataError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(SupplementBibliographicMetadataError::InvalidSession)?
        .to_str()
        .map_err(|_| SupplementBibliographicMetadataError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(SupplementBibliographicMetadataError::InvalidSession);
            }
            return Ok(session_token.to_string());
        }
    }

    Err(SupplementBibliographicMetadataError::InvalidSession)
}
