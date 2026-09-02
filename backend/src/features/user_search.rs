use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

const MAX_USER_SEARCH_RESULTS: i64 = 20;

#[derive(Debug, Deserialize)]
pub struct UserSearchQuery {
    pub user_name: String,
}

#[derive(Debug, Serialize)]
pub struct UserSearchItem {
    pub user_id: Uuid,
    pub user_name: String,
}

#[derive(Debug)]
enum UserSearchError {
    Database(sqlx::Error),
}

impl IntoResponse for UserSearchError {
    fn into_response(self) -> Response {
        match self {
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "internal_server_error",
                    "message": "Internal server error"
                })),
            )
                .into_response(),
        }
    }
}

/// Public lookup used only to resolve a visible user name to the stable user URI stored
/// in dcterms:creator. Email and other account data are never returned.
async fn search_users(
    State(state): State<AppState>,
    Query(query): Query<UserSearchQuery>,
) -> Result<Json<Vec<UserSearchItem>>, UserSearchError> {
    let user_name = query.user_name.trim();
    if user_name.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let pattern = format!("%{user_name}%");
    let rows = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        SELECT id, user_name
        FROM users
        WHERE user_name ILIKE $1
        ORDER BY
            CASE WHEN lower(user_name) = lower($2) THEN 0 ELSE 1 END,
            lower(user_name),
            id
        LIMIT $3
        "#,
    )
    .bind(pattern)
    .bind(user_name)
    .bind(MAX_USER_SEARCH_RESULTS)
    .fetch_all(&state.posgre)
    .await
    .map_err(UserSearchError::Database)?;

    Ok(Json(
        rows.into_iter()
            .map(|(user_id, user_name)| UserSearchItem { user_id, user_name })
            .collect(),
    ))
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/users/search", get(search_users))
        .with_state(state)
}
