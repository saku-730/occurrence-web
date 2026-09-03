use std::{collections::HashMap, sync::OnceLock};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode, header::COOKIE},
};
use serde::Serialize;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    features::auth::service::AuthService,
    state::AppState,
};

use super::{
    repository::PaperRepository,
    source_handler::{self, PaperSourceHandlerError},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionJobOccurrenceCandidate {
    pub scientific_name: String,
    pub locality: Option<String>,
    pub country: Option<String>,
    pub event_date: Option<String>,
    pub decimal_latitude: Option<f64>,
    pub decimal_longitude: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct StartExtractionJobResponse {
    pub source_kind: String,
    pub source_id: Uuid,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ExtractionJobStatusResponse {
    pub source_kind: String,
    pub source_id: Uuid,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrences: Option<Vec<ExtractionJobOccurrenceCandidate>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
enum ExtractionJobState {
    Processing,
    Completed(Vec<ExtractionJobOccurrenceCandidate>),
    Failed { error: String, message: String },
}

static EXTRACTION_JOBS: OnceLock<RwLock<HashMap<Uuid, ExtractionJobState>>> = OnceLock::new();

fn extraction_jobs() -> &'static RwLock<HashMap<Uuid, ExtractionJobState>> {
    EXTRACTION_JOBS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub async fn start_extraction(
    State(state): State<AppState>,
    AxumPath((source_kind, source_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<StartExtractionJobResponse>), PaperSourceHandlerError> {
    authenticate(&state, &headers).await?;
    if source_kind != "paper" {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    let paper_id =
        Uuid::parse_str(&source_id).map_err(|_| PaperSourceHandlerError::InvalidInput)?;
    PaperRepository::find_by_id(&state.posgre, paper_id)
        .await?
        .ok_or(PaperSourceHandlerError::NotFound)?;

    {
        let mut jobs = extraction_jobs().write().await;
        if matches!(jobs.get(&paper_id), Some(ExtractionJobState::Processing)) {
            return Ok((
                StatusCode::ACCEPTED,
                Json(StartExtractionJobResponse {
                    source_kind: "paper".to_string(),
                    source_id: paper_id,
                    status: "processing",
                }),
            ));
        }
        jobs.insert(paper_id, ExtractionJobState::Processing);
    }

    let task_state = state.clone();
    let task_headers = headers.clone();
    tokio::spawn(async move {
        let result = source_handler::extract_occurrences(
            State(task_state),
            AxumPath(("paper".to_string(), paper_id.to_string())),
            task_headers,
        )
        .await;

        let next_state = match result {
            Ok(Json(response)) => ExtractionJobState::Completed(
                response
                    .occurrences
                    .into_iter()
                    .map(|candidate| ExtractionJobOccurrenceCandidate {
                        scientific_name: candidate.scientific_name,
                        locality: candidate.locality,
                        country: candidate.country,
                        event_date: candidate.event_date,
                        decimal_latitude: candidate.decimal_latitude,
                        decimal_longitude: candidate.decimal_longitude,
                    })
                    .collect(),
            ),
            Err(error) => {
                let (error_code, message) = extraction_error_details(&error);
                eprintln!("paper occurrence extraction job failed: {paper_id} {error:?}");
                ExtractionJobState::Failed {
                    error: error_code.to_string(),
                    message: message.to_string(),
                }
            }
        };

        extraction_jobs().write().await.insert(paper_id, next_state);
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(StartExtractionJobResponse {
            source_kind: "paper".to_string(),
            source_id: paper_id,
            status: "processing",
        }),
    ))
}

pub async fn extraction_status(
    State(state): State<AppState>,
    AxumPath((source_kind, source_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ExtractionJobStatusResponse>, PaperSourceHandlerError> {
    authenticate(&state, &headers).await?;
    if source_kind != "paper" {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    let paper_id =
        Uuid::parse_str(&source_id).map_err(|_| PaperSourceHandlerError::InvalidInput)?;
    let job = extraction_jobs().read().await.get(&paper_id).cloned();

    let response = match job {
        None => ExtractionJobStatusResponse {
            source_kind: "paper".to_string(),
            source_id: paper_id,
            status: "not_started",
            occurrences: None,
            error: None,
            message: None,
        },
        Some(ExtractionJobState::Processing) => ExtractionJobStatusResponse {
            source_kind: "paper".to_string(),
            source_id: paper_id,
            status: "processing",
            occurrences: None,
            error: None,
            message: None,
        },
        Some(ExtractionJobState::Completed(occurrences)) => ExtractionJobStatusResponse {
            source_kind: "paper".to_string(),
            source_id: paper_id,
            status: "completed",
            occurrences: Some(occurrences),
            error: None,
            message: None,
        },
        Some(ExtractionJobState::Failed { error, message }) => ExtractionJobStatusResponse {
            source_kind: "paper".to_string(),
            source_id: paper_id,
            status: "failed",
            occurrences: None,
            error: Some(error),
            message: Some(message),
        },
    };

    Ok(Json(response))
}

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<(), PaperSourceHandlerError> {
    let token = extract_session_token(headers)?;
    AuthService::current_user(&state.posgre, token)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, PaperSourceHandlerError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(PaperSourceHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| PaperSourceHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            if token.trim().is_empty() {
                return Err(PaperSourceHandlerError::InvalidSession);
            }
            return Ok(token.to_string());
        }
    }

    Err(PaperSourceHandlerError::InvalidSession)
}

fn extraction_error_details(error: &PaperSourceHandlerError) -> (&'static str, &'static str) {
    match error {
        PaperSourceHandlerError::InvalidSession => ("invalid_session", "Invalid session"),
        PaperSourceHandlerError::InvalidInput => (
            "invalid_paper_source",
            "Invalid paper source request",
        ),
        PaperSourceHandlerError::NotFound => ("paper_not_found", "Paper not found"),
        PaperSourceHandlerError::UnsupportedMediaType => (
            "unsupported_media_type",
            "Only PDF files are accepted",
        ),
        PaperSourceHandlerError::PayloadTooLarge => (
            "payload_too_large",
            "PDF file exceeds the 100MB limit",
        ),
        PaperSourceHandlerError::ObjectStoreFailed => (
            "object_store_error",
            "Failed to read or store the paper PDF",
        ),
        PaperSourceHandlerError::GrobidFailed => (
            "grobid_error",
            "Failed to extract paper metadata with GROBID",
        ),
        PaperSourceHandlerError::ExtractionFailed => (
            "occurrence_extraction_error",
            "Failed to extract occurrences from the paper",
        ),
        PaperSourceHandlerError::Database(_) | PaperSourceHandlerError::FileSystem(_) => (
            "internal_server_error",
            "Internal server error",
        ),
    }
}
