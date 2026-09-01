use std::collections::HashSet;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    features::auth::{
        dto::ErrorResponse,
        service::{AuthService, AuthServiceError},
    },
    state::AppState,
};

use super::{
    extraction::{
        LlamaPaperOccurrenceExtractor, PaperOccurrenceExtractionError,
        PaperOccurrenceExtractionService,
    },
    llama::OccurrenceCandidate,
};

#[derive(Debug)]
pub enum ExtractOccurrencesHandlerError {
    InvalidSession,
    InvalidInput,
    NotFound,
    ObjectStoreFailed,
    ExtractionFailed,
    Database(sqlx::Error),
    FileSystem(std::io::Error),
}

impl From<AuthServiceError> for ExtractOccurrencesHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<PaperOccurrenceExtractionError> for ExtractOccurrencesHandlerError {
    fn from(error: PaperOccurrenceExtractionError) -> Self {
        match error {
            PaperOccurrenceExtractionError::NotFound => Self::NotFound,
            PaperOccurrenceExtractionError::ObjectStoreFailed
            | PaperOccurrenceExtractionError::InvalidStoredPdf => Self::ObjectStoreFailed,
            PaperOccurrenceExtractionError::Extractor(_) => Self::ExtractionFailed,
            PaperOccurrenceExtractionError::Database(error) => Self::Database(error),
            PaperOccurrenceExtractionError::FileSystem(error) => Self::FileSystem(error),
        }
    }
}

impl IntoResponse for ExtractOccurrencesHandlerError {
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
                    error: "invalid_paper_import".to_string(),
                    message: "Invalid paper import request".to_string(),
                }),
            )
                .into_response(),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "paper_import_not_found".to_string(),
                    message: "Paper import not found or not ready for extraction".to_string(),
                }),
            )
                .into_response(),
            Self::ObjectStoreFailed => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "object_store_error".to_string(),
                    message: "Failed to read the staged paper PDF".to_string(),
                }),
            )
                .into_response(),
            Self::ExtractionFailed => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "occurrence_extraction_error".to_string(),
                    message: "Failed to extract occurrences from the staged paper".to_string(),
                }),
            )
                .into_response(),
            Self::Database(_) | Self::FileSystem(_) => (
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

#[derive(Debug, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedOccurrenceCandidate {
    pub scientific_name: String,
    pub locality: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ExtractOccurrencesResponse {
    pub status: String,
    pub import_id: Uuid,
    pub occurrences: Vec<ExtractedOccurrenceCandidate>,
}

#[utoipa::path(
    post,
    path = "/paper-imports/{import_id}/extract-occurrences",
    params(("import_id" = String, Path, description = "Paper import UUID")),
    responses(
        (status = 200, description = "Occurrence candidates extracted for browser-side review", body = ExtractOccurrencesResponse),
        (status = 400, description = "Invalid paper import UUID", body = ErrorResponse),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 404, description = "Paper import not found or not ready", body = ErrorResponse),
        (status = 500, description = "Database or temporary file operation failed", body = ErrorResponse),
        (status = 502, description = "Garage read or llama.cpp extraction failed", body = ErrorResponse)
    ),
    tag = "paper-import"
)]
pub async fn extract_occurrences(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ExtractOccurrencesResponse>, ExtractOccurrencesHandlerError> {
    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;
    let import_id =
        Uuid::parse_str(&import_id).map_err(|_| ExtractOccurrencesHandlerError::InvalidInput)?;

    let output = PaperOccurrenceExtractionService::extract(
        import_id,
        current_user.user_id,
        state.media_object_store.as_ref(),
        &LlamaPaperOccurrenceExtractor,
        &state.posgre,
    )
    .await?;

    Ok(Json(ExtractOccurrencesResponse {
        status: "reviewing".to_string(),
        import_id: output.import_id,
        occurrences: frontend_occurrence_candidates(output.result.occurrences),
    }))
}

fn frontend_occurrence_candidates(
    candidates: Vec<OccurrenceCandidate>,
) -> Vec<ExtractedOccurrenceCandidate> {
    let mut seen = HashSet::new();

    candidates
        .into_iter()
        .filter_map(|candidate| {
            let scientific_name = candidate.scientific_name.trim().to_string();
            if scientific_name.is_empty() {
                return None;
            }

            let locality = candidate
                .locality
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let key = (scientific_name.clone(), locality.clone());

            seen.insert(key).then_some(ExtractedOccurrenceCandidate {
                scientific_name,
                locality,
            })
        })
        .collect()
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, ExtractOccurrencesHandlerError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(ExtractOccurrencesHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| ExtractOccurrencesHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(ExtractOccurrencesHandlerError::InvalidSession);
            }
            return Ok(session_token.to_string());
        }
    }

    Err(ExtractOccurrencesHandlerError::InvalidSession)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn candidate(scientific_name: &str, locality: Option<&str>) -> OccurrenceCandidate {
        OccurrenceCandidate {
            scientific_name: scientific_name.to_string(),
            locality: locality.map(ToString::to_string),
            decimal_latitude: None,
            decimal_longitude: None,
        }
    }

    #[test]
    fn frontend_candidates_are_trimmed_deduplicated_and_keep_first_order() {
        let candidates = frontend_occurrence_candidates(vec![
            candidate(" Metaphire hilgendorfi ", Some(" Tokyo ")),
            candidate("Metaphire hilgendorfi", Some("Tokyo")),
            candidate("Amynthas agrestis", Some("")),
            candidate("   ", Some("ignored")),
        ]);

        assert_eq!(
            candidates,
            vec![
                ExtractedOccurrenceCandidate {
                    scientific_name: "Metaphire hilgendorfi".to_string(),
                    locality: Some("Tokyo".to_string()),
                },
                ExtractedOccurrenceCandidate {
                    scientific_name: "Amynthas agrestis".to_string(),
                    locality: None,
                },
            ]
        );
    }

    #[test]
    fn extraction_response_serializes_only_frontend_candidate_fields() {
        let response = ExtractOccurrencesResponse {
            status: "reviewing".to_string(),
            import_id: Uuid::nil(),
            occurrences: vec![ExtractedOccurrenceCandidate {
                scientific_name: "Metaphire hilgendorfi".to_string(),
                locality: Some("Tokyo".to_string()),
            }],
        };

        let value = serde_json::to_value(response).expect("response should serialize");
        assert_eq!(
            value["occurrences"][0],
            json!({
                "scientificName": "Metaphire hilgendorfi",
                "locality": "Tokyo"
            })
        );
        assert!(value["occurrences"][0].get("decimalLatitude").is_none());
        assert!(value["occurrences"][0].get("decimalLongitude").is_none());
    }
}
