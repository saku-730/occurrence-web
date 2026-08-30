use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{patch, post},
};

pub mod dto;
pub mod extraction;
pub mod extraction_handler;
pub mod fulltext;
pub mod grobid;
mod grobid_client_api;
pub mod handler;
pub mod llama;
pub mod preprocess;
pub mod repository;
pub mod service;
pub mod source_handler;
pub mod staging;
pub mod staging_dto;
pub mod staging_handler;

// paper_importsは未正式登録PDFのsource情報を保持するために使う。
// 新フローではstaged/extracting/reviewingの状態遷移をAPIの条件にしない。
// 同一PDFはSHA-256で既存sourceを返し、Garage/PostgreSQLへ重複保存しない。
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
