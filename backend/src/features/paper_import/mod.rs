use axum::{Router, extract::DefaultBodyLimit, routing::post};

use crate::state::AppState;

pub mod dto;
pub mod handler;

// paper import機能のrouteを機能単位でまとめる。
// 現段階ではPDF受信・検証までを担当し、重複判定、永続化、GROBID処理は後続で追加する。
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/paper-import",
            post(handler::receive_pdf)
                .layer(DefaultBodyLimit::max(handler::PAPER_PDF_REQUEST_BODY_LIMIT_BYTES)),
        )
        .with_state(state)
}
