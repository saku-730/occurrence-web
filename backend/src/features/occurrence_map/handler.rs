use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use oxrdf::NamedNode;

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        occurrences::service::{
            OccurrenceServiceError, SearchOccurrenceFilterInput, SearchVisibility,
        },
    },
    state::AppState,
};

use super::{
    dto::{OccurrenceMapFeatureCollection, OccurrenceMapSearchRequest},
    service::list_occurrence_map,
};

#[derive(Debug)]
pub enum OccurrenceMapHandlerError {
    Database(sqlx::Error),
    InvalidSearchFilter,
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
            Self::InvalidSearchFilter => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_search_filter".to_string(),
                    message: "Invalid search filter".to_string(),
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
            description = "GeoJSON FeatureCollection of all viewable occurrences with complete coordinates",
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
    let visibility = resolve_visibility(&state, &headers).await?;
    let map =
        list_occurrence_map(state.occurrence_rdf_store.as_ref(), visibility, Vec::new()).await?;
    Ok(Json(map))
}

#[utoipa::path(
    post,
    path = "/occurrences/map/search",
    request_body(
        content = OccurrenceMapSearchRequest,
        content_type = "application/json",
        description = "Filter map occurrences with the same arbitrary predicate filters used by /occurrences/search. Multiple filters are ANDed."
    ),
    responses(
        (
            status = 200,
            description = "Filtered GeoJSON FeatureCollection of viewable occurrences with complete coordinates",
            body = OccurrenceMapFeatureCollection
        ),
        (status = 400, description = "Invalid search filter", body = ErrorResponse),
        (status = 502, description = "Failed to read occurrence RDF store", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "occurrences"
)]
pub async fn search_occurrence_map(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<OccurrenceMapSearchRequest>,
) -> Result<Json<OccurrenceMapFeatureCollection>, OccurrenceMapHandlerError> {
    let filters = normalize_search_filters(request)?;
    let visibility = resolve_visibility(&state, &headers).await?;
    let map = list_occurrence_map(state.occurrence_rdf_store.as_ref(), visibility, filters).await?;
    Ok(Json(map))
}

fn normalize_search_filters(
    request: OccurrenceMapSearchRequest,
) -> Result<Vec<SearchOccurrenceFilterInput>, OccurrenceMapHandlerError> {
    request
        .filters
        .into_iter()
        .map(|filter| {
            if !(filter.predicate.starts_with("http://")
                || filter.predicate.starts_with("https://"))
                || NamedNode::new(filter.predicate.as_str()).is_err()
            {
                return Err(OccurrenceMapHandlerError::InvalidSearchFilter);
            }
            if filter.value_type != "literal" && filter.value_type != "uri" {
                return Err(OccurrenceMapHandlerError::InvalidSearchFilter);
            }
            if filter.r#match != "exact" {
                return Err(OccurrenceMapHandlerError::InvalidSearchFilter);
            }
            if filter.value_type == "uri" && NamedNode::new(filter.value.as_str()).is_err() {
                return Err(OccurrenceMapHandlerError::InvalidSearchFilter);
            }

            Ok(SearchOccurrenceFilterInput {
                predicate: filter.predicate,
                value: filter.value,
                value_type: filter.value_type,
                match_type: filter.r#match,
            })
        })
        .collect()
}

async fn resolve_visibility(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<SearchVisibility, OccurrenceMapHandlerError> {
    match optional_session_token(headers) {
        None => Ok(SearchVisibility::PublicOnly),
        Some(session_token) => {
            match AuthService::current_user(&state.posgre, session_token).await {
                Ok(current_user) if current_user.role == "admin" => Ok(SearchVisibility::All),
                Ok(current_user) => Ok(SearchVisibility::PublicOrOwnPrivate {
                    user_id: current_user.user_id,
                }),
                Err(AuthServiceError::InvalidSession) => Ok(SearchVisibility::PublicOnly),
                Err(AuthServiceError::Database(error)) => {
                    Err(OccurrenceMapHandlerError::Database(error))
                }
                Err(_) => Ok(SearchVisibility::PublicOnly),
            }
        }
    }
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
