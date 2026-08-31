use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{patch, post},
};

pub mod extraction;
pub mod fulltext;
pub mod grobid;
mod grobid_client_api;
pub mod llama;
pub mod preprocess;
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
        .route(
            "/paper-sources/{source_kind}/{source_id}/bibliographic-metadata",
            patch(source_handler::update_bibliographic_metadata),
        )
        .route(
            "/paper-sources/{source_kind}/{source_id}/extract-occurrences",
            post(source_handler::extract_occurrences),
        )
        .with_state(state)
}
