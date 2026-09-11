use axum::{Router, routing::get};

use crate::state::AppState;

pub mod dto;
pub mod handler;
pub mod repository;
pub mod service;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/label-templates",
            get(handler::list_label_templates).post(handler::create_label_template),
        )
        .route(
            "/label-templates/{template_id}",
            get(handler::get_label_template)
                .put(handler::update_label_template)
                .delete(handler::delete_label_template),
        )
        .with_state(state)
}
