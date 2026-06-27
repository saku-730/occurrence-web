use axum::{
    Json,
    extract::{Multipart, State},
    http::{
        HeaderMap, StatusCode,
        header::{CONTENT_LENGTH, COOKIE},
    },
    response::{IntoResponse, Response},
};

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        media::{
            dto::UploadMediaResponse,
            service::{MediaService, MediaServiceError, UploadMediaInput},
        },
    },
    state::AppState,
};

#[derive(Debug)]
pub enum MediaHandlerError {
    InvalidSession,
    InvalidInput,
    PayloadTooLarge,
    ObjectStoreFailed,
    Database(sqlx::Error),
}

impl From<AuthServiceError> for MediaHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<MediaServiceError> for MediaHandlerError {
    fn from(error: MediaServiceError) -> Self {
        match error {
            MediaServiceError::InvalidInput => Self::InvalidInput,
            MediaServiceError::PayloadTooLarge => Self::PayloadTooLarge,
            MediaServiceError::ObjectStoreFailed => Self::ObjectStoreFailed,
        }
    }
}

impl IntoResponse for MediaHandlerError {
    fn into_response(self) -> Response {
        match self {
            MediaHandlerError::InvalidSession => (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "invalid_session".to_string(),
                    message: "Invalid session".to_string(),
                }),
            )
                .into_response(),
            MediaHandlerError::InvalidInput => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_media".to_string(),
                    message: "Invalid media".to_string(),
                }),
            )
                .into_response(),
            MediaHandlerError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ErrorResponse {
                    error: "payload_too_large".to_string(),
                    message: "Payload too large".to_string(),
                }),
            )
                .into_response(),
            MediaHandlerError::ObjectStoreFailed => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "object_store_error".to_string(),
                    message: "Object store error".to_string(),
                }),
            )
                .into_response(),
            MediaHandlerError::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                }),
            )
                .into_response(),
        }
    }
}

pub async fn upload_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadMediaResponse>), MediaHandlerError> {
    reject_oversized_request_by_content_length(&headers)?;

    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;

    let mut file_name = None;
    let mut content_type = None;
    let mut bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| MediaHandlerError::InvalidInput)?
    {
        if field.name() != Some("file") {
            continue;
        }

        file_name = field.file_name().map(ToString::to_string);
        content_type = field.content_type().map(ToString::to_string);
        bytes = Some(
            field
                .bytes()
                .await
                .map_err(|_| MediaHandlerError::InvalidInput)?
                .to_vec(),
        );
        break;
    }

    let bytes = bytes.ok_or(MediaHandlerError::InvalidInput)?;
    let content_type = content_type.ok_or(MediaHandlerError::InvalidInput)?;

    // bucketは当面MVP仕様の固定値を使う。S3_BUCKETのConfig化はobject store本番実装時にまとめる。
    let output = MediaService::upload_media(
        UploadMediaInput {
            app_base_url: state.config.app.app_base_url.clone(),
            bucket: "occurrence-media".to_string(),
            uploaded_by: current_user.user_id,
            original_filename: file_name,
            content_type,
            bytes,
        },
        state.media_object_store.as_ref(),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(UploadMediaResponse {
            media_id: output.media_id,
            media_uri: output.media_uri,
            bucket: output.bucket,
            object_key: output.object_key,
            content_type: output.content_type,
            size_bytes: output.size_bytes,
            original_filename: output.original_filename,
        }),
    ))
}

const MEDIA_REQUEST_LIMIT_BYTES: u64 = 1000 * 1024 * 1024;

fn reject_oversized_request_by_content_length(
    headers: &HeaderMap,
) -> Result<(), MediaHandlerError> {
    let Some(content_length) = headers.get(CONTENT_LENGTH) else {
        return Ok(());
    };

    let content_length = content_length
        .to_str()
        .map_err(|_| MediaHandlerError::InvalidInput)?
        .parse::<u64>()
        .map_err(|_| MediaHandlerError::InvalidInput)?;

    // multipart全体のContent-Lengthが最大メディア上限を超えるなら、bodyを読まずに拒否する。
    // spec上の最大は動画の1000MBなので、入口では全content typeに対して1000MBを共通上限にする。
    if content_length > MEDIA_REQUEST_LIMIT_BYTES {
        return Err(MediaHandlerError::PayloadTooLarge);
    }

    Ok(())
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, MediaHandlerError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(MediaHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| MediaHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(MediaHandlerError::InvalidSession);
            }
            return Ok(session_token.to_string());
        }
    }

    Err(MediaHandlerError::InvalidSession)
}
