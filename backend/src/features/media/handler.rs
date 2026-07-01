use axum::{
    Json,
    extract::{Multipart, State, multipart::Field},
    http::{
        HeaderMap, StatusCode,
        header::{CONTENT_LENGTH, COOKIE},
    },
    response::{IntoResponse, Response},
};

use sha2::{Digest, Sha256};
use tempfile::TempPath;
use tokio::io::AsyncWriteExt;

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        media::{
            dto::{UploadMediaRequest, UploadMediaResponse},
            service::{
                MEDIA_FILE_SIZE_LIMIT_BYTES, MediaService, MediaServiceError, UploadMediaInput,
            },
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
    FileSystem(std::io::Error),
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
            MediaServiceError::Database(error) => Self::Database(error),
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
            MediaHandlerError::Database(_) | MediaHandlerError::FileSystem(_) => (
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

#[utoipa::path(
    post,
    path = "/media",
    request_body(
        content = UploadMediaRequest,
        content_type = "multipart/form-data",
        description = "Authenticated media upload. The file field accepts jpg/jpeg, png, webp, mp3, wav, m4a, mp4, or mov up to 1000MB."
    ),
    responses(
        (
            status = 201,
            description = "Media object and metadata created, or existing media reused for the same user and SHA-256",
            body = UploadMediaResponse
        ),
        (
            status = 400,
            description = "Invalid multipart body, MIME type, detected file format, or filename extension",
            body = ErrorResponse
        ),
        (
            status = 401,
            description = "Authentication required",
            body = ErrorResponse
        ),
        (
            status = 413,
            description = "Media file exceeds the 1000MB limit",
            body = ErrorResponse
        ),
        (
            status = 500,
            description = "PostgreSQL or temporary file operation failed",
            body = ErrorResponse
        ),
        (
            status = 502,
            description = "Garage object storage operation failed",
            body = ErrorResponse
        )
    ),
    tag = "media"
)]
pub async fn upload_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadMediaResponse>), MediaHandlerError> {
    reject_oversized_request_by_content_length(&headers)?;

    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;

    let mut prepared_upload = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| MediaHandlerError::InvalidInput)?
    {
        if field.name() != Some("file") {
            continue;
        }

        let original_filename = field.file_name().map(ToString::to_string);
        let content_type = field
            .content_type()
            .map(ToString::to_string)
            .ok_or(MediaHandlerError::InvalidInput)?;
        let staged_file = stage_field_to_temporary_file(&mut field).await?;
        prepared_upload = Some((original_filename, content_type, staged_file));
        break;
    }

    let (original_filename, content_type, staged_file) =
        prepared_upload.ok_or(MediaHandlerError::InvalidInput)?;
    let StagedMediaFile {
        temp_path,
        size_bytes,
        payload_sha256,
        mime_probe,
    } = staged_file;

    // 起動時に検証済みのS3_BUCKETを使い、環境ごとのbucket名をhandlerへ固定しない。
    let upload_result = MediaService::upload_media(
        UploadMediaInput {
            app_base_url: state.config.app.app_base_url.clone(),
            bucket: state.config.garage.bucket.clone(),
            uploaded_by: current_user.user_id,
            original_filename,
            content_type,
            file_path: temp_path.to_path_buf(),
            size_bytes,
            payload_sha256,
            mime_probe,
        },
        state.media_object_store.as_ref(),
        &state.posgre,
    )
    .await;

    // serviceが成功・失敗のどちらでも、responseを返す前に一時ファイルを削除する。
    // service側エラーがある場合は元の原因を優先し、cleanup失敗で上書きしない。
    let cleanup_result = temp_path.close();
    let output = match upload_result {
        Ok(output) => {
            cleanup_result.map_err(MediaHandlerError::FileSystem)?;
            output
        }
        Err(error) => {
            let _ = cleanup_result;
            return Err(error.into());
        }
    };

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

const MULTIPART_OVERHEAD_ALLOWANCE_BYTES: usize = 1024 * 1024;
pub const MEDIA_REQUEST_BODY_LIMIT_BYTES: usize =
    MEDIA_FILE_SIZE_LIMIT_BYTES as usize + MULTIPART_OVERHEAD_ALLOWANCE_BYTES;
const MIME_PROBE_LIMIT_BYTES: usize = 8192;

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

    // multipart全体にはboundary/headerが含まれるため、ファイル上限に1MiBの余裕を加えたrequest上限で早期拒否する。
    // Content-Lengthは信用せず、実ファイル上限はchunk読込中のsize_bytesで別途強制する。
    if content_length > MEDIA_REQUEST_BODY_LIMIT_BYTES as u64 {
        return Err(MediaHandlerError::PayloadTooLarge);
    }

    Ok(())
}

struct StagedMediaFile {
    temp_path: TempPath,
    size_bytes: u64,
    payload_sha256: String,
    mime_probe: Vec<u8>,
}

async fn stage_field_to_temporary_file(
    field: &mut Field<'_>,
) -> Result<StagedMediaFile, MediaHandlerError> {
    // tempfileが安全な一意名を作り、TempPathのDropでも削除されるためearly return時も残留しない。
    let temp_path = tempfile::Builder::new()
        .prefix("occurrence-media-upload-")
        .tempfile()
        .map_err(MediaHandlerError::FileSystem)?
        .into_temp_path();
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&temp_path)
        .await
        .map_err(MediaHandlerError::FileSystem)?;
    let mut hasher = Sha256::new();
    let mut size_bytes = 0_u64;
    let mut mime_probe = Vec::with_capacity(MIME_PROBE_LIMIT_BYTES);

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| MediaHandlerError::InvalidInput)?
    {
        size_bytes = size_bytes
            .checked_add(chunk.len() as u64)
            .ok_or(MediaHandlerError::PayloadTooLarge)?;
        if size_bytes > MEDIA_FILE_SIZE_LIMIT_BYTES {
            return Err(MediaHandlerError::PayloadTooLarge);
        }

        hasher.update(&chunk);
        if mime_probe.len() < MIME_PROBE_LIMIT_BYTES {
            let remaining = MIME_PROBE_LIMIT_BYTES - mime_probe.len();
            mime_probe.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        output
            .write_all(&chunk)
            .await
            .map_err(MediaHandlerError::FileSystem)?;
    }

    output
        .flush()
        .await
        .map_err(MediaHandlerError::FileSystem)?;
    drop(output);

    Ok(StagedMediaFile {
        temp_path,
        size_bytes,
        payload_sha256: hex::encode(hasher.finalize()),
        mime_probe,
    })
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
