use axum::{
    body::Bytes,
    extract::State,
    http::{
        header::{CONTENT_TYPE,COOKIE,},
        HeaderMap,
        StatusCode,
    },
    response::{IntoResponse, Response},
    Json,
};

use crate::{
    features::auth::{
        dto::ErrorResponse,
        service::{AuthService, AuthServiceError},
    },
    state::AppState,
};

#[derive(Debug)]
pub enum OccurrenceHandlerError {
    InvalidSession, //セッションを持っていないなど
    Database(sqlx::Error),   //posgre側のエラー トークン認証など
    NotImplemented, //
    UnsupportedMediaType, //httpリクエストのbodyがtext/turtle以外など
}

impl From<AuthServiceError> for OccurrenceHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl IntoResponse for OccurrenceHandlerError {
    fn into_response(self) -> Response {
        match self {
            OccurrenceHandlerError::InvalidSession => {
                let body = ErrorResponse {
                    error: "invalid_session".to_string(),
                    message: "Invalid session".to_string(),
                };

                (StatusCode::UNAUTHORIZED, Json(body)).into_response()
            }
            
            OccurrenceHandlerError::Database(_) => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
            OccurrenceHandlerError::NotImplemented => {
                let body = ErrorResponse {
                    error: "not_implemented".to_string(),
                    message: "Occurrence creation is not implemented yet".to_string(),
                };

                (StatusCode::NOT_IMPLEMENTED, Json(body)).into_response()
            }
            OccurrenceHandlerError::UnsupportedMediaType => {
                let body = ErrorResponse {
                    error: "unsupported_media_type".to_string(),
                    message: "Unsupported media type".to_string(),
                };

                (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(body)).into_response()
            }
        }
    }
}

pub async fn create_occurrence(
    State(state): State<AppState>,
    headers: HeaderMap,
    _body: Bytes,
) -> Result<StatusCode, OccurrenceHandlerError> {
    let session_token = extract_session_token(&headers)?;

    let _current_user = AuthService::current_user(
        &state.posgre,
        session_token,
    )
    .await?;

    ensure_supported_rdf_content_type(&headers)?;

    Err(OccurrenceHandlerError::NotImplemented)
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, OccurrenceHandlerError> { //トークン取り出し ヘルパー
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(OccurrenceHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| OccurrenceHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') { //session=asdfasdf; user=asdfasdf;...って感じのヘッダー
        let cookie = cookie.trim(); //cookie整形

        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(OccurrenceHandlerError::InvalidSession);
            }

            return Ok(session_token.to_string());
        }
    }

    Err(OccurrenceHandlerError::InvalidSession)
}

fn ensure_supported_rdf_content_type( //content-typeを確認 text/turtle以外はエラー
    headers: &HeaderMap,
) -> Result<(), OccurrenceHandlerError> {
    let content_type = headers
        .get(CONTENT_TYPE)
        .ok_or(OccurrenceHandlerError::UnsupportedMediaType)?
        .to_str()
        .map_err(|_| OccurrenceHandlerError::UnsupportedMediaType)?;

    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match media_type.as_str() {
        "text/turtle" => Ok(()),
        _ => Err(OccurrenceHandlerError::UnsupportedMediaType),
    }
}