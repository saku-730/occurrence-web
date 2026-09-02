use axum::{Router, routing::{get, post}};

use crate::state::AppState;

pub mod dto;
pub mod geocoding;
pub mod handler;
pub mod location_store;
pub mod service;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/occurrences/map", get(handler::get_occurrence_map))
        .route("/occurrences/map/search", post(handler::search_occurrence_map))
        .with_state(state)
}
