use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use serde::Serialize;
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

#[derive(Debug, Serialize)]
pub struct ExtractOccurrencesResponse {
    pub status: String,
    pub import_id: Uuid,
    pub occurrences: Vec<OccurrenceCandidate>,
}

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
        occurrences: output.result.occurrences,
    }))
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
