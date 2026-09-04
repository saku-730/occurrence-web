use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, patch, post},
};

pub mod extraction;
pub mod extraction_job_handler;
pub mod fulltext;
pub mod gbif;
pub mod gbif_handler;
pub mod grobid;
mod grobid_client_api;
pub mod list_handler;
pub mod llama;
pub mod preprocess;
pub mod registration_handler;
pub mod registration_job_handler;
pub mod repository;
pub mod service;
pub mod source_handler;

// papers is the single PostgreSQL source of truth for paper PDFs.
// status is intentionally limited to unregistered/registered occurrence data.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/paper-import",
            post(source_handler::receive_pdf).layer(DefaultBodyLimit::max(
                source_handler::PAPER_SOURCE_PDF_REQUEST_BODY_LIMIT_BYTES,
            )),
        )
        .route("/papers", get(list_handler::list_papers))
        .route(
            "/paper-sources/{source_kind}/{source_id}/bibliographic-metadata",
            patch(source_handler::update_bibliographic_metadata),
        )
        .route(
            "/paper-sources/{source_kind}/{source_id}/extract-occurrences",
            post(extraction_job_handler::start_extraction),
        )
        .route(
            "/paper-sources/{source_kind}/{source_id}/extract-occurrences/status",
            get(extraction_job_handler::extraction_status),
        )
        .route(
            "/papers/{paper_id}/resolve-taxa",
            post(gbif_handler::resolve_taxa),
        )
        .route(
            "/papers/{paper_id}/occurrences",
            post(registration_handler::register_occurrence),
        )
        .route(
            "/papers/{paper_id}/occurrences/batch",
            post(registration_handler::register_occurrences_batch),
        )
        .route(
            "/papers/{paper_id}/occurrences/batch-jobs",
            post(registration_job_handler::start_registration),
        )
        .route(
            "/papers/{paper_id}/occurrences/batch-jobs/status",
            get(registration_job_handler::registration_status),
        )
        .with_state(state)
}
