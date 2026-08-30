use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, patch, post},
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

// 新しいpaper importフローではpaper_importsを処理状態のstate machineとして扱わない。
// PDFが初回ならGarage + paper_importsへ1度だけ保存し、同一PDFが既に存在する場合は
// SHA-256で既存sourceを返して再利用する。Occurrence抽出可否はstatus列で判定しない。
// 旧paper-imports endpointは既存互換のため一旦残す。
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
        .route(
            "/paper-imports/{import_id}/bibliographic-metadata",
            patch(staging_handler::complete_bibliographic_metadata),
        )
        .route(
            "/paper-imports/{import_id}/extract-occurrences",
            post(extraction_handler::extract_occurrences),
        )
        .route(
            "/paper-imports/{import_id}",
            delete(staging_handler::cancel_import),
        )
        .route(
            "/papers/{paper_id}/bibliographic-metadata",
            patch(handler::complete_bibliographic_metadata),
        )
        .with_state(state)
}
