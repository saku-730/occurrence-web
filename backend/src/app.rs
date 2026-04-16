use axum::{routing::get, Router};

pub fn build_app() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
}

async fn index() -> &'static str {
    "Occurrence App Backend"
}

async fn health() -> &'static str {
    "ok"
}