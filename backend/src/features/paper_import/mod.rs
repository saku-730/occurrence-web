use axum::{Router, extract::DefaultBodyLimit, routing::{patch, post}};

use crate::state::AppState;

pub mod dto;
pub mod grobid;
mod grobid_client_api;
pub mod handler;
pub mod repository;
pub mod service;
pub mod supplement;

// paper import機能のrouteを機能単位でまとめる。
// PDF受信、重複判定、Garage保存、GROBID metadata抽出、PostgreSQL保存と、
// GROBIDで最低限の書誌情報が取れなかったpaperのユーザー補完を担当する。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/paper-import",
            post(handler::receive_pdf)
                .layer(DefaultBodyLimit::max(handler::PAPER_PDF_REQUEST_BODY_LIMIT_BYTES)),
        )
        .route(
            "/papers/{paper_id}/bibliographic-metadata",
            patch(supplement::supplement_bibliographic_metadata),
        )
        .with_state(state)
}
