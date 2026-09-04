use std::{collections::HashMap, sync::OnceLock};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header::COOKIE},
};
use serde::Serialize;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    features::{
        auth::service::AuthService,
        occurrence_map::geocoding::enrich_nquads_with_geocoding_and_abr,
    },
    infrastructure::{abr::AbrClient, nominatim::NominatimClient},
    state::AppState,
};

use super::{
    registration_handler::{
        self, PaperRegistrationError, RegisterPaperOccurrencesBatchRequest,
    },
    repository::PaperRepository,
};

const MAX_BATCH_OCCURRENCES: usize = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct RegistrationJobOccurrence {
    pub occurrence_id: String,
    pub occurrence_uri: String,
}

#[derive(Debug, Serialize)]
pub struct StartRegistrationJobResponse {
    pub paper_id: Uuid,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RegistrationJobStatusResponse {
    pub paper_id: Uuid,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processed_occurrences: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_occurrences: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrences: Option<Vec<RegistrationJobOccurrence>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
enum RegistrationJobState {
    Processing {
        phase: String,
        processed_occurrences: usize,
        total_occurrences: usize,
    },
    Completed(Vec<RegistrationJobOccurrence>),
    Failed {
        error: String,
        message: String,
    },
}

static REGISTRATION_JOBS: OnceLock<RwLock<HashMap<Uuid, RegistrationJobState>>> = OnceLock::new();

fn registration_jobs() -> &'static RwLock<HashMap<Uuid, RegistrationJobState>> {
    REGISTRATION_JOBS.get_or_init(|| RwLock::new(HashMap::new()))
}

pub async fn start_registration(
    State(state): State<AppState>,
    Path(paper_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<RegisterPaperOccurrencesBatchRequest>,
) -> Result<(StatusCode, Json<StartRegistrationJobResponse>), PaperRegistrationError> {
    if request.occurrences.is_empty() || request.occurrences.len() > MAX_BATCH_OCCURRENCES {
        return Err(PaperRegistrationError::InvalidInput);
    }
    if request.occurrences.iter().any(|nquads| nquads.trim().is_empty()) {
        return Err(PaperRegistrationError::InvalidInput);
    }

    authenticate(&state, &headers).await?;
    PaperRepository::find_by_id(&state.posgre, paper_id)
        .await?
        .ok_or(PaperRegistrationError::NotFound)?;

    let total_occurrences = request.occurrences.len();
    {
        let mut jobs = registration_jobs().write().await;
        if matches!(jobs.get(&paper_id), Some(RegistrationJobState::Processing { .. })) {
            return Ok((
                StatusCode::ACCEPTED,
                Json(StartRegistrationJobResponse {
                    paper_id,
                    status: "processing",
                }),
            ));
        }
        jobs.insert(
            paper_id,
            RegistrationJobState::Processing {
                phase: "geocoding".to_string(),
                processed_occurrences: 0,
                total_occurrences,
            },
        );
    }

    let task_state = state.clone();
    let task_headers = headers.clone();
    tokio::spawn(async move {
        let geocoder = NominatimClient::global();
        let abr = AbrClient::global();
        let mut enriched_occurrences = Vec::with_capacity(total_occurrences);

        for (index, nquads) in request.occurrences.into_iter().enumerate() {
            let enriched = match enrich_nquads_with_geocoding_and_abr(
                nquads.as_bytes(),
                geocoder,
                abr,
            )
            .await
            {
                Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|_| nquads.clone()),
                Err(_) => nquads,
            };
            enriched_occurrences.push(enriched);

            registration_jobs().write().await.insert(
                paper_id,
                RegistrationJobState::Processing {
                    phase: "geocoding".to_string(),
                    processed_occurrences: index + 1,
                    total_occurrences,
                },
            );
        }

        registration_jobs().write().await.insert(
            paper_id,
            RegistrationJobState::Processing {
                phase: "registering".to_string(),
                processed_occurrences: 0,
                total_occurrences,
            },
        );

        let result = registration_handler::register_occurrences_batch(
            State(task_state),
            Path(paper_id),
            task_headers,
            Json(RegisterPaperOccurrencesBatchRequest {
                occurrences: enriched_occurrences,
            }),
        )
        .await;

        let next_state = match result {
            Ok((_status, Json(response))) => RegistrationJobState::Completed(
                response
                    .occurrences
                    .into_iter()
                    .map(|occurrence| RegistrationJobOccurrence {
                        occurrence_id: occurrence.occurrence_id,
                        occurrence_uri: occurrence.occurrence_uri,
                    })
                    .collect(),
            ),
            Err(error) => {
                let (error_code, message) = registration_error_details(&error);
                eprintln!("paper occurrence registration job failed: {paper_id} {error:?}");
                RegistrationJobState::Failed {
                    error: error_code.to_string(),
                    message: message.to_string(),
                }
            }
        };

        registration_jobs().write().await.insert(paper_id, next_state);
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(StartRegistrationJobResponse {
            paper_id,
            status: "processing",
        }),
    ))
}

pub async fn registration_status(
    State(state): State<AppState>,
    Path(paper_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<RegistrationJobStatusResponse>, PaperRegistrationError> {
    authenticate(&state, &headers).await?;

    let job = registration_jobs().read().await.get(&paper_id).cloned();
    let response = match job {
        None => RegistrationJobStatusResponse {
            paper_id,
            status: "not_started",
            phase: None,
            processed_occurrences: None,
            total_occurrences: None,
            occurrences: None,
            error: None,
            message: None,
        },
        Some(RegistrationJobState::Processing {
            phase,
            processed_occurrences,
            total_occurrences,
        }) => RegistrationJobStatusResponse {
            paper_id,
            status: "processing",
            phase: Some(phase),
            processed_occurrences: Some(processed_occurrences),
            total_occurrences: Some(total_occurrences),
            occurrences: None,
            error: None,
            message: None,
        },
        Some(RegistrationJobState::Completed(occurrences)) => RegistrationJobStatusResponse {
            paper_id,
            status: "completed",
            phase: None,
            processed_occurrences: Some(occurrences.len()),
            total_occurrences: Some(occurrences.len()),
            occurrences: Some(occurrences),
            error: None,
            message: None,
        },
        Some(RegistrationJobState::Failed { error, message }) => RegistrationJobStatusResponse {
            paper_id,
            status: "failed",
            phase: None,
            processed_occurrences: None,
            total_occurrences: None,
            occurrences: None,
            error: Some(error),
            message: Some(message),
        },
    };

    Ok(Json(response))
}

async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<(), PaperRegistrationError> {
    let token = extract_session_token(headers)?;
    AuthService::current_user(&state.posgre, token)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, PaperRegistrationError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(PaperRegistrationError::InvalidSession)?
        .to_str()
        .map_err(|_| PaperRegistrationError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            if token.trim().is_empty() {
                return Err(PaperRegistrationError::InvalidSession);
            }
            return Ok(token.to_string());
        }
    }

    Err(PaperRegistrationError::InvalidSession)
}

fn registration_error_details(error: &PaperRegistrationError) -> (&'static str, &'static str) {
    match error {
        PaperRegistrationError::InvalidSession => ("invalid_session", "Invalid session"),
        PaperRegistrationError::InvalidInput => (
            "invalid_paper_occurrence",
            "Invalid paper occurrence request",
        ),
        PaperRegistrationError::NotFound => ("paper_not_found", "Paper not found"),
        PaperRegistrationError::InvalidRdf => ("invalid_rdf", "Invalid occurrence RDF body"),
        PaperRegistrationError::ForbiddenMedia => (
            "forbidden_media",
            "Occurrence media must be owned by the authenticated user",
        ),
        PaperRegistrationError::StoreFailed => (
            "rdf_store_error",
            "Failed to save occurrence RDF",
        ),
        PaperRegistrationError::Database(_) | PaperRegistrationError::Internal => (
            "internal_server_error",
            "Internal server error",
        ),
    }
}
