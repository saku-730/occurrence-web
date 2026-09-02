use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        occurrences::service::{OccurrenceServiceError, SearchVisibility},
    },
    state::AppState,
};

use super::{dto::OccurrenceMapFeatureCollection, service::list_occurrence_map};

#[derive(Debug)]
pub enum OccurrenceMapHandlerError {
    Database(sqlx::Error),
    StoreFailed,
}

impl From<OccurrenceServiceError> for OccurrenceMapHandlerError {
    fn from(_: OccurrenceServiceError) -> Self {
        Self::StoreFailed
    }
}

impl IntoResponse for OccurrenceMapHandlerError {
    fn into_response(self) -> Response {
        match self {
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                }),
            )
                .into_response(),
            Self::StoreFailed => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "rdf_store_error".to_string(),
                    message: "Failed to load occurrence map data".to_string(),
                }),
            )
                .into_response(),
        }
    }
}

#[utoipa::path(
    get,
    path = "/occurrences/map",
    responses(
        (
            status = 200,
            description = "GeoJSON FeatureCollection of viewable occurrences with complete coordinates",
            body = OccurrenceMapFeatureCollection
        ),
        (
            status = 502,
            description = "Failed to read occurrence RDF store",
            body = ErrorResponse
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse
        )
    ),
    tag = "occurrences"
)]
pub async fn get_occurrence_map(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<OccurrenceMapFeatureCollection>, OccurrenceMapHandlerError> {
    let visibility = match optional_session_token(&headers) {
        None => SearchVisibility::PublicOnly,
        Some(session_token) => match AuthService::current_user(&state.posgre, session_token).await {
            Ok(current_user) if current_user.role == "admin" => SearchVisibility::All,
            Ok(current_user) => SearchVisibility::PublicOrOwnPrivate {
                user_id: current_user.user_id,
            },
            Err(AuthServiceError::InvalidSession) => SearchVisibility::PublicOnly,
            Err(AuthServiceError::Database(error)) => {
                return Err(OccurrenceMapHandlerError::Database(error));
            }
            Err(_) => SearchVisibility::PublicOnly,
        },
    };

    let map = list_occurrence_map(state.occurrence_rdf_store.as_ref(), visibility).await?;
    Ok(Json(map))
}

fn optional_session_token(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(COOKIE)?.to_str().ok()?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}
