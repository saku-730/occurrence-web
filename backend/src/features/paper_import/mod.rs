use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, patch, post},
};

pub mod dto;
pub mod fulltext;
pub mod grobid;
mod grobid_client_api;
pub mod handler;
pub mod llama;
pub mod preprocess;
pub mod repository;
pub mod service;
pub mod staging;
pub mod staging_dto;
pub mod staging_handler;

// PDF受信からユーザー確認まではpaper_imports + Garage上の仮PDFとして保持する。
// papersへの正式登録はOccurrenceの確定処理と同じタイミングで行う。
// 旧handler/serviceは既存テストと既に正式登録済みpaperの補完互換のため一旦残す。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/paper-import",
            post(staging_handler::receive_pdf).layer(DefaultBodyLimit::max(
                staging_handler::PAPER_PDF_REQUEST_BODY_LIMIT_BYTES,
            )),
        )
        .route(
            "/paper-imports/{import_id}/bibliographic-metadata",
            patch(staging_handler::complete_bibliographic_metadata),
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
