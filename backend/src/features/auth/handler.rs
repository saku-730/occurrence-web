use axum::{
    http::StatusCode,
    response::IntoResponse,
};

pub async fn register() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "register not implemented")
}

