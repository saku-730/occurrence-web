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
    service::PAPER_PDF_FILE_SIZE_LIMIT_BYTES,
    staging::{
        CompleteStagedBibliographicMetadataInput, PaperImportStagingError,
        PaperImportStagingService, StartPaperImportInput, StartPaperImportStatus,
    },
    staging_dto::{
        CancelPaperImportResponse, CompleteStagedBibliographicMetadataRequest,
        CompleteStagedBibliographicMetadataResponse, StartPaperImportRequest,
        StartPaperImportResponse,
    },
};

const MULTIPART_OVERHEAD_ALLOWANCE_BYTES: usize = 1024 * 1024;
pub const PAPER_PDF_REQUEST_BODY_LIMIT_BYTES: usize =
    PAPER_PDF_FILE_SIZE_LIMIT_BYTES as usize + MULTIPART_OVERHEAD_ALLOWANCE_BYTES;
const PDF_SIGNATURE: &[u8] = b"%PDF-";

#[derive(Debug)]
pub enum StagedPaperImportHandlerError {
    InvalidSession,
    InvalidInput,
    NotFound,
    UnsupportedMediaType,
    PayloadTooLarge,
    ObjectStoreFailed,
    GrobidFailed,
    Database(sqlx::Error),
    FileSystem(std::io::Error),
}

impl From<AuthServiceError> for StagedPaperImportHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<PaperImportStagingError> for StagedPaperImportHandlerError {
    fn from(error: PaperImportStagingError) -> Self {
        match error {
            PaperImportStagingError::InvalidInput => Self::InvalidInput,
            PaperImportStagingError::NotFound => Self::NotFound,
            PaperImportStagingError::ObjectStoreFailed => Self::ObjectStoreFailed,
            PaperImportStagingError::Grobid(_) => Self::GrobidFailed,
            PaperImportStagingError::Database(error) => Self::Database(error),
        }
    }
}

impl IntoResponse for StagedPaperImportHandlerError {
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
                    error: "invalid_paper_import".to_string(),
                    message: "Invalid paper import request".to_string(),
                }),
            )
                .into_response(),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "paper_import_not_found".to_string(),
                    message: "Paper import not found".to_string(),
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
                    message: "Failed to stage paper PDF".to_string(),
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
            Self::Database(_) | Self::FileSystem(_) => (
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
        content = StartPaperImportRequest,
        content_type = "multipart/form-data",
        description = "Start a paper import. The PDF is staged for review; no papers row is created until the occurrence import is committed."
    ),
    responses(
        (status = 201, description = "PDF staged and GROBID metadata extracted", body = StartPaperImportResponse),
        (status = 200, description = "The identical PDF is already formally registered", body = StartPaperImportResponse),
        (status = 400, description = "Invalid upload", body = ErrorResponse),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 413, description = "PDF exceeds the 100MB limit", body = ErrorResponse),
        (status = 415, description = "Uploaded file is not a PDF", body = ErrorResponse),
        (status = 500, description = "PostgreSQL or temporary file operation failed", body = ErrorResponse),
        (status = 502, description = "Garage staging or GROBID operation failed", body = ErrorResponse)
    ),
    tag = "paper-import"
)]
pub async fn receive_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<StartPaperImportResponse>), StagedPaperImportHandlerError> {
    reject_oversized_request_by_content_length(&headers)?;

    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;

    let mut received_pdf = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| StagedPaperImportHandlerError::InvalidInput)?
    {
        if field.name() != Some("file") {
            continue;
        }

        if received_pdf.is_some() {
            return Err(StagedPaperImportHandlerError::InvalidInput);
        }

        let original_filename = field.file_name().map(ToString::to_string);
        validate_filename(original_filename.as_deref())?;

        let content_type = field
            .content_type()
            .map(ToString::to_string)
            .ok_or(StagedPaperImportHandlerError::UnsupportedMediaType)?;
        if !content_type.trim().eq_ignore_ascii_case("application/pdf") {
            return Err(StagedPaperImportHandlerError::UnsupportedMediaType);
        }

        let staged = stage_pdf_to_temporary_file(&mut field).await?;
        received_pdf = Some((original_filename, content_type, staged));
    }

    let (original_filename, content_type, staged) =
        received_pdf.ok_or(StagedPaperImportHandlerError::InvalidInput)?;

    let StagedPaperPdf {
        temp_path,
        size_bytes,
        sha256,
    } = staged;

    let start_result = PaperImportStagingService::start(
        StartPaperImportInput {
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

    let cleanup_result = temp_path.close();
    let output = match start_result {
        Ok(output) => {
            cleanup_result.map_err(StagedPaperImportHandlerError::FileSystem)?;
            output
        }
        Err(error) => {
            let _ = cleanup_result;
            return Err(error.into());
        }
    };

    let (http_status, status, message) = match output.status {
        StartPaperImportStatus::Staged => (
            StatusCode::CREATED,
            "staged",
            "paper PDF staged; no formal paper has been registered yet",
        ),
        StartPaperImportStatus::MetadataRequired => (
            StatusCode::CREATED,
            "metadata_required",
            "paper PDF staged; DOI or title must be supplied before extraction continues",
        ),
        StartPaperImportStatus::AlreadyImported => (
            StatusCode::OK,
            "already_imported",
            "paper PDF is already formally registered",
        ),
    };

    Ok((
        http_status,
        Json(StartPaperImportResponse {
            status: status.to_string(),
            import_id: output.import_id,
            paper_id: output.paper_id,
            original_filename: output.original_filename,
            content_type: output.content_type,
            size_bytes: output.size_bytes,
            sha256: output.sha256,
            doi: output.doi,
            title: output.title,
            requires_bibliographic_input: output.requires_bibliographic_input,
            authors: output.authors,
            publication_year: output.publication_year,
            journal: output.journal,
            volume: output.volume,
            issue: output.issue,
            pages: output.pages,
            article_number: output.article_number,
            message: message.to_string(),
        }),
    ))
}

#[utoipa::path(
    patch,
    path = "/paper-imports/{import_id}/bibliographic-metadata",
    params(("import_id" = String, Path, description = "Paper import UUID")),
    request_body = CompleteStagedBibliographicMetadataRequest,
    responses(
        (status = 200, description = "Missing DOI or title completed on the staged import", body = CompleteStagedBibliographicMetadataResponse),
        (status = 400, description = "Invalid import UUID or empty bibliographic input", body = ErrorResponse),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 404, description = "Paper import not found", body = ErrorResponse),
        (status = 500, description = "PostgreSQL operation failed", body = ErrorResponse)
    ),
    tag = "paper-import"
)]
pub async fn complete_bibliographic_metadata(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CompleteStagedBibliographicMetadataRequest>,
) -> Result<Json<CompleteStagedBibliographicMetadataResponse>, StagedPaperImportHandlerError> {
    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;
    let import_id =
        Uuid::parse_str(&import_id).map_err(|_| StagedPaperImportHandlerError::InvalidInput)?;

    let output = PaperImportStagingService::complete_bibliographic_metadata(
        CompleteStagedBibliographicMetadataInput {
            import_id,
            uploaded_by: current_user.user_id,
            doi: request.doi,
            title: request.title,
        },
        &state.posgre,
    )
    .await?;

    Ok(Json(CompleteStagedBibliographicMetadataResponse {
        status: if output.requires_bibliographic_input {
            "metadata_required".to_string()
        } else {
            "staged".to_string()
        },
        import_id: output.import_id,
        doi: output.doi,
        title: output.title,
        requires_bibliographic_input: output.requires_bibliographic_input,
        message: "bibliographic metadata updated on staged paper import".to_string(),
    }))
}

#[utoipa::path(
    delete,
    path = "/paper-imports/{import_id}",
    params(("import_id" = String, Path, description = "Paper import UUID")),
    responses(
        (status = 200, description = "Staged import and staged PDF removed", body = CancelPaperImportResponse),
        (status = 400, description = "Invalid import UUID", body = ErrorResponse),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 404, description = "Paper import not found", body = ErrorResponse),
        (status = 500, description = "PostgreSQL operation failed", body = ErrorResponse),
        (status = 502, description = "Failed to remove staged PDF from Garage", body = ErrorResponse)
    ),
    tag = "paper-import"
)]
pub async fn cancel_import(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<CancelPaperImportResponse>, StagedPaperImportHandlerError> {
    let session_token = extract_session_token(&headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;
    let import_id =
        Uuid::parse_str(&import_id).map_err(|_| StagedPaperImportHandlerError::InvalidInput)?;

    PaperImportStagingService::cancel(
        import_id,
        current_user.user_id,
        state.media_object_store.as_ref(),
        &state.posgre,
    )
    .await?;

    Ok(Json(CancelPaperImportResponse {
        status: "cancelled".to_string(),
        import_id,
        message: "staged paper import removed".to_string(),
    }))
}

struct StagedPaperPdf {
    temp_path: TempPath,
    size_bytes: u64,
    sha256: String,
}

async fn stage_pdf_to_temporary_file(
    field: &mut Field<'_>,
) -> Result<StagedPaperPdf, StagedPaperImportHandlerError> {
    let temp_path = tempfile::Builder::new()
        .prefix("paper-import-")
        .suffix(".pdf")
        .tempfile()
        .map_err(StagedPaperImportHandlerError::FileSystem)?
        .into_temp_path();

    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&temp_path)
        .await
        .map_err(StagedPaperImportHandlerError::FileSystem)?;

    let mut hasher = Sha256::new();
    let mut size_bytes = 0_u64;
    let mut signature = Vec::with_capacity(PDF_SIGNATURE.len());

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| StagedPaperImportHandlerError::InvalidInput)?
    {
        size_bytes = size_bytes
            .checked_add(chunk.len() as u64)
            .ok_or(StagedPaperImportHandlerError::PayloadTooLarge)?;

        if size_bytes > PAPER_PDF_FILE_SIZE_LIMIT_BYTES {
            return Err(StagedPaperImportHandlerError::PayloadTooLarge);
        }

        if signature.len() < PDF_SIGNATURE.len() {
            let remaining = PDF_SIGNATURE.len() - signature.len();
            signature.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }

        hasher.update(&chunk);
        output
            .write_all(&chunk)
            .await
            .map_err(StagedPaperImportHandlerError::FileSystem)?;
    }

    if size_bytes == 0 {
        return Err(StagedPaperImportHandlerError::InvalidInput);
    }

    if signature.as_slice() != PDF_SIGNATURE {
        return Err(StagedPaperImportHandlerError::UnsupportedMediaType);
    }

    output
        .flush()
        .await
        .map_err(StagedPaperImportHandlerError::FileSystem)?;
    drop(output);

    Ok(StagedPaperPdf {
        temp_path,
        size_bytes,
        sha256: hex::encode(hasher.finalize()),
    })
}

fn validate_filename(filename: Option<&str>) -> Result<(), StagedPaperImportHandlerError> {
    let filename = filename.ok_or(StagedPaperImportHandlerError::InvalidInput)?;
    let is_pdf = std::path::Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));

    if !is_pdf {
        return Err(StagedPaperImportHandlerError::UnsupportedMediaType);
    }

    Ok(())
}

fn reject_oversized_request_by_content_length(
    headers: &HeaderMap,
) -> Result<(), StagedPaperImportHandlerError> {
    let Some(content_length) = headers.get(CONTENT_LENGTH) else {
        return Ok(());
    };

    let content_length = content_length
        .to_str()
        .map_err(|_| StagedPaperImportHandlerError::InvalidInput)?
        .parse::<u64>()
        .map_err(|_| StagedPaperImportHandlerError::InvalidInput)?;

    if content_length > PAPER_PDF_REQUEST_BODY_LIMIT_BYTES as u64 {
        return Err(StagedPaperImportHandlerError::PayloadTooLarge);
    }

    Ok(())
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, StagedPaperImportHandlerError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(StagedPaperImportHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| StagedPaperImportHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(StagedPaperImportHandlerError::InvalidSession);
            }
            return Ok(session_token.to_string());
        }
    }

    Err(StagedPaperImportHandlerError::InvalidSession)
}
