use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    features::auth::{
        dto::ErrorResponse,
        service::{AuthService, AuthServiceError},
    },
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct ListPapersQuery {
    pub q: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PaperListItem {
    pub id: uuid::Uuid,
    pub title: Option<String>,
    pub doi: Option<String>,
    pub first_imported_at: String,
}

#[derive(Debug, Serialize)]
pub struct ListPapersResponse {
    pub papers: Vec<PaperListItem>,
}

#[derive(Debug)]
pub enum ListPapersError {
    InvalidSession,
    Database(sqlx::Error),
}

impl From<AuthServiceError> for ListPapersError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl IntoResponse for ListPapersError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidSession => (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_session".to_string(),
                    message: "Invalid session".to_string(),
                }),
            )
                .into_response(),
            Self::Database(error) => {
                eprintln!("paper list query failed: {error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "internal_server_error".to_string(),
                        message: "Internal server error".to_string(),
                    }),
                )
                    .into_response()
            }
        }
    }
}

pub async fn list_papers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListPapersQuery>,
) -> Result<Json<ListPapersResponse>, ListPapersError> {
    let session_token = extract_session_token(&headers)?;
    AuthService::current_user(&state.posgre, session_token).await?;

    let search = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let papers = sqlx::query_as::<_, PaperListItem>(
        r#"
        SELECT
            id,
            title,
            doi,
            to_char(
                created_at AT TIME ZONE 'UTC',
                'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'
            ) AS first_imported_at
        FROM papers
        WHERE
            $1::text IS NULL
            OR title ILIKE '%' || $1 || '%'
            OR doi ILIKE '%' || $1 || '%'
        ORDER BY created_at DESC, id DESC
        "#,
    )
    .bind(search)
    .fetch_all(&state.posgre)
    .await
    .map_err(ListPapersError::Database)?;

    Ok(Json(ListPapersResponse { papers }))
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, ListPapersError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(ListPapersError::InvalidSession)?
        .to_str()
        .map_err(|_| ListPapersError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            if token.trim().is_empty() {
                return Err(ListPapersError::InvalidSession);
            }
            return Ok(token.to_string());
        }
    }

    Err(ListPapersError::InvalidSession)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn extracts_session_cookie_from_multiple_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("theme=dark; session=test-token; other=value"),
        );

        assert_eq!(extract_session_token(&headers).unwrap(), "test-token");
    }

    #[test]
    fn rejects_missing_session_cookie() {
        let headers = HeaderMap::new();
        assert!(matches!(
            extract_session_token(&headers),
            Err(ListPapersError::InvalidSession)
        ));
    }
}
