use axum::{routing::get, Router};

use crate::state::AppState;

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .with_state(state)
}

async fn index() -> &'static str {
    "Occurrence App Backend"
}

async fn health() -> &'static str {
    "ok"
}