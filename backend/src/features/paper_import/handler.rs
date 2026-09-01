use axum::{
    Json,
    extract::{Multipart, Path, State, multipart::Field},
    http::{
        HeaderMap, StatusCode,
        header::{CONTENT_LENGTH, COOKIE},
    },
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use tempfile::TempPath;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    features::auth::{
        dto::ErrorResponse,
        service::{AuthService, AuthServiceError},
    },
    state::AppState,
};

use super::{
    dto::{
        CompleteBibliographicMetadataRequest, CompleteBibliographicMetadataResponse,
        ReceivePaperPdfRequest, ReceivePaperPdfResponse,
    },
    service::{
        CompleteBibliographicMetadataInput, ImportPaperPdfInput, ImportPaperPdfStatus,
        PaperImportService, PaperImportServiceError,
    },
};

pub const PAPER_PDF_FILE_SIZE_LIMIT_BYTES: u64 = super::service::PAPER_PDF_FILE_SIZE_LIMIT_BYTES;
const MULTIPART_OVERHEAD_ALLOWANCE_BYTES: usize = 1024 * 1024;
pub const PAPER_PDF_REQUEST_BODY_LIMIT_BYTES: usize =
    PAPER_PDF_FILE_SIZE_LIMIT_BYTES as usize + MULTIPART_OVERHEAD_ALLOWANCE_BYTES;
const PDF_SIGNATURE: &[u8] = b"%PDF-";

#[derive(Debug)]
pub enum PaperImportHandlerError {
    InvalidSession,
    InvalidInput,
    NotFound,
    UnsupportedMediaType,
    PayloadTooLarge,
    ObjectStoreFailed,
    GrobidFailed,
    Database(sqlx::Error),
    FileSystem(std::io::Error),
    Internal,
}

impl From<AuthServiceError> for PaperImportHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<PaperImportServiceError> for PaperImportHandlerError {
    fn from(error: PaperImportServiceError) -> Self {
        match error {
            PaperImportServiceError::InvalidInput => Self::InvalidInput,
            PaperImportServiceError::NotFound => Self::NotFound,
            PaperImportServiceError::ObjectStoreFailed => Self::ObjectStoreFailed,
            PaperImportServiceError::Grobid(_) => Self::GrobidFailed,
            PaperImportServiceError::Database(error) => Self::Database(error),
            PaperImportServiceError::ConflictResolutionFailed => Self::Internal,
        }
    }
}

impl IntoResponse for PaperImportHandlerError {
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
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "invalid_pdf".to_string(),
                    message: "Invalid PDF upload".to_string(),
                }),
            )
                .into_response(),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "paper_not_found".to_string(),
                    message: "Paper not found".to_string(),
                }),
            )
                .into_response(),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(ErrorResponse {
                    error: "unsupported_media_type".to_string(),
                    message: "Only PDF files are accepted".to_string(),
                }),
            )
                .into_response(),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ErrorResponse {
                    error: "payload_too_large".to_string(),
                    message: "PDF file exceeds the 100MB limit".to_string(),
                }),
            )
                .into_response(),
            Self::ObjectStoreFailed => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "object_store_error".to_string(),
                    message: "Failed to store paper PDF".to_string(),
                }),
            )
                .into_response(),
            Self::GrobidFailed => (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: "grobid_error".to_string(),
                    message: "Failed to extract paper metadata with GROBID".to_string(),
                }),
            )
                .into_response(),
            Self::Database(_) | Self::FileSystem(_) | Self::Internal => (
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
    path = "/paper-import",
    request_body(
        content = ReceivePaperPdfRequest,
        content_type = "multipart/form-data",
        description = "Authenticated paper PDF import. The multipart field name must be `file`. Only PDF files up to 100MB are accepted."
    ),
    responses(
        (
            status = 201,
            description = "New PDF stored in Garage, GROBID metadata saved when available, and paper registered in PostgreSQL. If both DOI and title are missing, status is metadata_required.",
            body = ReceivePaperPdfResponse
        ),
        (
            status = 200,
            description = "The identical PDF has already been imported; no new Garage object or GROBID request is created. If both DOI and title are missing, status is metadata_required.",
            body = ReceivePaperPdfResponse
        ),
        (
            status = 400,
            description = "Missing file field, empty file, invalid multipart body, or invalid filename",
            body = ErrorResponse
        ),
        (
            status = 401,
            description = "Authentication required",
            body = ErrorResponse
        ),
        (
            status = 413,
            description = "PDF exceeds the 100MB limit",
            body = ErrorResponse
        ),
        (
            status = 415,
            description = "Uploaded file is not a PDF",
            body = ErrorResponse
        ),
        (
            status = 500,
            description = "PostgreSQL or temporary file operation failed",
            body = ErrorResponse
        ),
        (
            status = 502,
            description = "Garage or GROBID operation failed",
            body = ErrorResponse
        )
    ),
    tag = "paper-import"
)]
pub async fn receive_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ReceivePaperPdfResponse>), PaperImportHandlerError> {
    reject_oversized_request_by_content_length(&headers)?;

    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;

    let mut received_pdf = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| PaperImportHandlerError::InvalidInput)?
    {
        if field.name() != Some("file") {
            continue;
        }

        // The API accepts exactly one PDF. Continue parsing after staging it so
        // duplicate file fields and malformed trailing multipart data cannot be
        // silently ignored.
        if received_pdf.is_some() {
            return Err(PaperImportHandlerError::InvalidInput);
        }

        let original_filename = field.file_name().map(ToString::to_string);
        validate_filename(original_filename.as_deref())?;

        let content_type = field
            .content_type()
            .map(ToString::to_string)
            .ok_or(PaperImportHandlerError::UnsupportedMediaType)?;
        if !content_type.trim().eq_ignore_ascii_case("application/pdf") {
            return Err(PaperImportHandlerError::UnsupportedMediaType);
        }

        let staged = stage_pdf_to_temporary_file(&mut field).await?;
        received_pdf = Some((original_filename, content_type, staged));
    }

    let (original_filename, content_type, staged) =
        received_pdf.ok_or(PaperImportHandlerError::InvalidInput)?;

    let StagedPaperPdf {
        temp_path,
        size_bytes,
        sha256,
    } = staged;

    let import_result = PaperImportService::import_pdf(
        ImportPaperPdfInput {
            bucket: state.config.garage.bucket.clone(),
            uploaded_by: current_user.user_id,
            original_filename,
            content_type,
            file_path: temp_path.to_path_buf(),
            size_bytes,
            payload_sha256: sha256,
        },
        state.media_object_store.as_ref(),
        &state.posgre,
    )
    .await;

    // 成功・失敗のどちらでもrequest終了前に一時PDFを削除する。
    // import本体のエラーがある場合はcleanupエラーで上書きしない。
    let cleanup_result = temp_path.close();
    let output = match import_result {
        Ok(output) => {
            cleanup_result.map_err(PaperImportHandlerError::FileSystem)?;
            output
        }
        Err(error) => {
            let _ = cleanup_result;
            return Err(error.into());
        }
    };

    let (http_status, status, message) = match output.status {
        ImportPaperPdfStatus::Imported => (
            StatusCode::CREATED,
            "imported",
            "paper PDF imported and metadata extracted",
        ),
        ImportPaperPdfStatus::MetadataRequired => (
            if output.already_imported {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            },
            "metadata_required",
            if output.already_imported {
                "paper PDF already imported; DOI or title must be supplied"
            } else {
                "paper PDF imported; DOI or title must be supplied"
            },
        ),
        ImportPaperPdfStatus::AlreadyImported => (
            StatusCode::OK,
            "already_imported",
            "paper PDF already imported",
        ),
    };

    Ok((
        http_status,
        Json(ReceivePaperPdfResponse {
            status: status.to_string(),
            paper_id: output.paper_id,
            original_filename: output.original_filename,
            content_type: output.content_type,
            size_bytes: output.size_bytes,
            sha256: output.sha256,
            doi: output.doi,
            title: output.title,
            authors: output.authors,
            publication_year: output.publication_year,
            journal: output.journal,
            volume: output.volume,
            issue: output.issue,
            pages: output.pages,
            article_number: output.article_number,
            requires_bibliographic_input: output.requires_bibliographic_input,
            message: message.to_string(),
        }),
    ))
}

#[utoipa::path(
    patch,
    path = "/papers/{paper_id}/bibliographic-metadata",
    params(
        ("paper_id" = String, Path, description = "Paper UUID")
    ),
    request_body = CompleteBibliographicMetadataRequest,
    responses(
        (status = 200, description = "Missing DOI or title completed", body = CompleteBibliographicMetadataResponse),
        (status = 400, description = "Invalid paper UUID or empty bibliographic input", body = ErrorResponse),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 404, description = "Paper not found", body = ErrorResponse),
        (status = 500, description = "PostgreSQL operation failed", body = ErrorResponse)
    ),
    tag = "paper-import"
)]
pub async fn complete_bibliographic_metadata(
    State(state): State<AppState>,
    Path(paper_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CompleteBibliographicMetadataRequest>,
) -> Result<Json<CompleteBibliographicMetadataResponse>, PaperImportHandlerError> {
    // Authenticate before parsing or looking up the identifier. Unauthenticated
    // callers therefore cannot use validation differences to probe papers.
    let session_token = extract_session_token(&headers)?;
    AuthService::current_user(&state.posgre, session_token).await?;
    let paper_id = Uuid::parse_str(&paper_id).map_err(|_| PaperImportHandlerError::InvalidInput)?;

    let output = PaperImportService::complete_bibliographic_metadata(
        CompleteBibliographicMetadataInput {
            paper_id,
            doi: request.doi,
            title: request.title,
        },
        &state.posgre,
    )
    .await?;

    Ok(Json(CompleteBibliographicMetadataResponse {
        status: "completed".to_string(),
        paper_id: output.paper_id,
        doi: output.doi,
        title: output.title,
        requires_bibliographic_input: output.requires_bibliographic_input,
        message: "bibliographic metadata completed".to_string(),
    }))
}

struct StagedPaperPdf {
    temp_path: TempPath,
    size_bytes: u64,
    sha256: String,
}

async fn stage_pdf_to_temporary_file(
    field: &mut Field<'_>,
) -> Result<StagedPaperPdf, PaperImportHandlerError> {
    let temp_path = tempfile::Builder::new()
        .prefix("paper-import-")
        .suffix(".pdf")
        .tempfile()
        .map_err(PaperImportHandlerError::FileSystem)?
        .into_temp_path();

    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&temp_path)
        .await
        .map_err(PaperImportHandlerError::FileSystem)?;

    let mut hasher = Sha256::new();
    let mut size_bytes = 0_u64;
    let mut signature = Vec::with_capacity(PDF_SIGNATURE.len());

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| PaperImportHandlerError::InvalidInput)?
    {
        size_bytes = size_bytes
            .checked_add(chunk.len() as u64)
            .ok_or(PaperImportHandlerError::PayloadTooLarge)?;

        if size_bytes > PAPER_PDF_FILE_SIZE_LIMIT_BYTES {
            return Err(PaperImportHandlerError::PayloadTooLarge);
        }

        if signature.len() < PDF_SIGNATURE.len() {
            let remaining = PDF_SIGNATURE.len() - signature.len();
            signature.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }

        hasher.update(&chunk);
        output
            .write_all(&chunk)
            .await
            .map_err(PaperImportHandlerError::FileSystem)?;
    }

    if size_bytes == 0 {
        return Err(PaperImportHandlerError::InvalidInput);
    }

    if signature.as_slice() != PDF_SIGNATURE {
        return Err(PaperImportHandlerError::UnsupportedMediaType);
    }

    output
        .flush()
        .await
        .map_err(PaperImportHandlerError::FileSystem)?;
    drop(output);

    Ok(StagedPaperPdf {
        temp_path,
        size_bytes,
        sha256: hex::encode(hasher.finalize()),
    })
}

fn validate_filename(filename: Option<&str>) -> Result<(), PaperImportHandlerError> {
    let filename = filename.ok_or(PaperImportHandlerError::InvalidInput)?;
    let is_pdf = std::path::Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));

    if !is_pdf {
        return Err(PaperImportHandlerError::UnsupportedMediaType);
    }

    Ok(())
}

fn reject_oversized_request_by_content_length(
    headers: &HeaderMap,
) -> Result<(), PaperImportHandlerError> {
    let Some(content_length) = headers.get(CONTENT_LENGTH) else {
        return Ok(());
    };

    let content_length = content_length
        .to_str()
        .map_err(|_| PaperImportHandlerError::InvalidInput)?
        .parse::<u64>()
        .map_err(|_| PaperImportHandlerError::InvalidInput)?;

    if content_length > PAPER_PDF_REQUEST_BODY_LIMIT_BYTES as u64 {
        return Err(PaperImportHandlerError::PayloadTooLarge);
    }

    Ok(())
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, PaperImportHandlerError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(PaperImportHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| PaperImportHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(PaperImportHandlerError::InvalidSession);
            }
            return Ok(session_token.to_string());
        }
    }

    Err(PaperImportHandlerError::InvalidSession)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bibliographic_metadata_database_failure_maps_to_500() {
        let response = PaperImportHandlerError::from(PaperImportServiceError::Database(
            sqlx::Error::RowNotFound,
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
