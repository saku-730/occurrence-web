use axum::{Router, extract::DefaultBodyLimit, routing::post};

use crate::state::AppState;

pub mod dto;
pub mod grobid;
pub mod handler;
pub mod repository;
pub mod service;

// paper import機能のrouteを機能単位でまとめる。
// PDF受信、重複判定、Garage保存、GROBID metadata抽出、PostgreSQL保存までを担当する。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/paper-import",
            post(handler::receive_pdf)
                .layer(DefaultBodyLimit::max(handler::PAPER_PDF_REQUEST_BODY_LIMIT_BYTES)),
        )
        .with_state(state)
}
