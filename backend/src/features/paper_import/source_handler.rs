use std::path::Path;

use axum::{
    Json,
    extract::{Multipart, Path as AxumPath, State, multipart::Field},
    http::{
        HeaderMap, StatusCode,
        header::{CONTENT_LENGTH, COOKIE},
    },
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::TempPath;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError, CurrentUserOutput},
        },
        media::service::{
            DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectStore, PutMediaObjectInput,
        },
    },
    state::AppState,
};

use super::{
    extraction::{LlamaPaperOccurrenceExtractor, PaperOccurrenceExtractor},
    grobid::{GrobidClient, normalize_doi},
    llama::PaperLlmExtractionError,
    repository::{
        InsertPaperMetadata, PAPER_STATUS_REGISTERED, PaperMetadata, PaperRepository,
    },
    service::PAPER_PDF_FILE_SIZE_LIMIT_BYTES,
};

const PDF_SIGNATURE: &[u8] = b"%PDF-";
const MULTIPART_OVERHEAD_ALLOWANCE_BYTES: usize = 1024 * 1024;
pub const PAPER_SOURCE_PDF_REQUEST_BODY_LIMIT_BYTES: usize =
    PAPER_PDF_FILE_SIZE_LIMIT_BYTES as usize + MULTIPART_OVERHEAD_ALLOWANCE_BYTES;

#[derive(Debug)]
pub enum PaperSourceHandlerError {
    InvalidSession,
    InvalidInput,
    NotFound,
    UnsupportedMediaType,
    PayloadTooLarge,
    ObjectStoreFailed,
    GrobidFailed,
    ExtractionFailed,
    Database(sqlx::Error),
    FileSystem(std::io::Error),
}

impl From<AuthServiceError> for PaperSourceHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidSession => Self::InvalidSession,
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<sqlx::Error> for PaperSourceHandlerError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for PaperSourceHandlerError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::InvalidSession => (
                StatusCode::UNAUTHORIZED,
                "invalid_session",
                "Invalid session",
            ),
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                "invalid_paper_source",
                "Invalid paper source request",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "paper_not_found",
                "Paper not found",
            ),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "Only PDF files are accepted",
            ),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                "PDF file exceeds the 100MB limit",
            ),
            Self::ObjectStoreFailed => (
                StatusCode::BAD_GATEWAY,
                "object_store_error",
                "Failed to read or store the paper PDF",
            ),
            Self::GrobidFailed => (
                StatusCode::BAD_GATEWAY,
                "grobid_error",
                "Failed to extract paper metadata with GROBID",
            ),
            Self::ExtractionFailed => (
                StatusCode::BAD_GATEWAY,
                "occurrence_extraction_error",
                "Failed to extract occurrences from the paper",
            ),
            Self::Database(_) | Self::FileSystem(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_server_error",
                "Internal server error",
            ),
        };

        (
            status,
            Json(ErrorResponse {
                error: error.to_string(),
                message: message.to_string(),
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
pub struct ReceivePaperSourceResponse {
    // Kept for frontend compatibility. It is true only when the PDF has already
    // produced registered occurrence data and the user should be asked whether to continue.
    pub duplicate: bool,
    pub source_kind: String,
    pub source_id: Uuid,
    pub status: String,
    pub original_filename: Option<String>,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub requires_bibliographic_input: bool,
    pub authors: Option<String>,
    pub publication_year: Option<i32>,
    pub journal: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub article_number: Option<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePaperSourceMetadataRequest {
    pub doi: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdatePaperSourceMetadataResponse {
    pub source_kind: String,
    pub source_id: Uuid,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub requires_bibliographic_input: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperSourceOccurrenceCandidate {
    pub scientific_name: String,
    pub locality: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExtractPaperSourceOccurrencesResponse {
    pub source_kind: String,
    pub source_id: Uuid,
    pub occurrences: Vec<PaperSourceOccurrenceCandidate>,
}

#[derive(Debug)]
struct ReceivedPdf {
    temp_path: TempPath,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug)]
struct SourceObjectRow {
    bucket: String,
    object_key: String,
    size_bytes: i64,
    sha256: String,
}

pub async fn receive_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ReceivePaperSourceResponse>), PaperSourceHandlerError> {
    reject_oversized_request_by_content_length(&headers)?;
    let user = authenticated_user(&state, &headers).await?;

    let mut received = None;
    let mut original_filename = None;
    let mut content_type = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| PaperSourceHandlerError::InvalidInput)?
    {
        if field.name() != Some("file") {
            continue;
        }
        if received.is_some() {
            return Err(PaperSourceHandlerError::InvalidInput);
        }

        let filename = field.file_name().map(ToString::to_string);
        validate_filename(filename.as_deref())?;
        let field_content_type = field
            .content_type()
            .map(ToString::to_string)
            .ok_or(PaperSourceHandlerError::UnsupportedMediaType)?;
        if !field_content_type
            .trim()
            .eq_ignore_ascii_case("application/pdf")
        {
            return Err(PaperSourceHandlerError::UnsupportedMediaType);
        }

        original_filename = filename;
        content_type = Some(field_content_type);
        received = Some(receive_to_temporary_file(&mut field).await?);
    }

    let received = received.ok_or(PaperSourceHandlerError::InvalidInput)?;
    let content_type = content_type.ok_or(PaperSourceHandlerError::InvalidInput)?;

    let result = receive_pdf_inner(
        &state,
        user.user_id,
        original_filename,
        content_type,
        &received,
    )
    .await;

    let cleanup = received.temp_path.close();
    match result {
        Ok(response) => {
            cleanup.map_err(PaperSourceHandlerError::FileSystem)?;
            Ok(response)
        }
        Err(error) => {
            let _ = cleanup;
            Err(error)
        }
    }
}

async fn receive_pdf_inner(
    state: &AppState,
    uploaded_by: Uuid,
    original_filename: Option<String>,
    content_type: String,
    received: &ReceivedPdf,
) -> Result<(StatusCode, Json<ReceivePaperSourceResponse>), PaperSourceHandlerError> {
    // 1. SHA-256 duplicate check comes before any permanent write.
    if let Some(existing) =
        PaperRepository::find_by_sha256(&state.posgre, &received.sha256).await?
    {
        if existing.status == PAPER_STATUS_REGISTERED {
            // Registered means occurrence data was successfully persisted at least once.
            // Do not change it back to unregistered when the user reprocesses this PDF.
            return Ok((
                StatusCode::OK,
                Json(response_from_paper(existing, true)),
            ));
        }

        // An unregistered paper already has a permanent Garage object and papers row.
        // Reuse both and make GROBID metadata extraction a best-effort retry.
        let paper = run_grobid_and_fill_metadata(state, existing.id, received).await?;
        return Ok((
            StatusCode::OK,
            Json(response_from_paper(paper, false)),
        ));
    }

    // 2. First upload: create the real paper UUID immediately. There is no reserved ID.
    let paper_id = Uuid::new_v4();
    let object_key = format!("papers/{paper_id}/original.pdf");
    let size_bytes =
        i64::try_from(received.size_bytes).map_err(|_| PaperSourceHandlerError::InvalidInput)?;

    // 3. Permanently store the PDF in Garage.
    state
        .media_object_store
        .put_object(PutMediaObjectInput {
            bucket: state.config.garage.bucket.clone(),
            object_key: object_key.clone(),
            content_type: content_type.clone(),
            file_path: received.temp_path.to_path_buf(),
            size_bytes: received.size_bytes,
            payload_sha256: received.sha256.clone(),
        })
        .await
        .map_err(|_| PaperSourceHandlerError::ObjectStoreFailed)?;

    // 4. Insert papers(status=unregistered). If the DB write fails, remove the
    // just-created Garage object so no orphaned PDF is left behind.
    let inserted = PaperRepository::insert_unregistered_if_sha256_absent(
        &state.posgre,
        InsertPaperMetadata {
            id: paper_id,
            bucket: &state.config.garage.bucket,
            object_key: &object_key,
            content_type: &content_type,
            size_bytes,
            original_filename: original_filename.as_deref(),
            sha256: &received.sha256,
            uploaded_by,
        },
    )
    .await;

    let inserted = match inserted {
        Ok(inserted) => inserted,
        Err(error) => {
            delete_just_uploaded_pdf(state, object_key).await?;
            return Err(PaperSourceHandlerError::Database(error));
        }
    };

    if !inserted {
        // Another request inserted the same SHA-256 between our initial check and
        // INSERT. Our Garage object is redundant, so delete it and reuse the winner.
        delete_just_uploaded_pdf(state, object_key).await?;
        let existing = PaperRepository::find_by_sha256(&state.posgre, &received.sha256)
            .await?
            .ok_or(PaperSourceHandlerError::NotFound)?;

        if existing.status == PAPER_STATUS_REGISTERED {
            return Ok((
                StatusCode::OK,
                Json(response_from_paper(existing, true)),
            ));
        }

        let paper = run_grobid_and_fill_metadata(state, existing.id, received).await?;
        return Ok((
            StatusCode::OK,
            Json(response_from_paper(paper, false)),
        ));
    }

    // 5. GROBID metadata extraction is best effort. The PDF and papers row are
    // already permanent; if GROBID is unavailable, return the paper with NULL
    // bibliographic fields so the frontend can ask for title or DOI.
    let paper = run_grobid_and_fill_metadata(state, paper_id, received).await?;

    Ok((
        StatusCode::CREATED,
        Json(response_from_paper(paper, false)),
    ))
}

async fn run_grobid_and_fill_metadata(
    state: &AppState,
    paper_id: Uuid,
    received: &ReceivedPdf,
) -> Result<PaperMetadata, PaperSourceHandlerError> {
    let metadata = match GrobidClient::from_env() {
        Ok(grobid) => match grobid
            .extract_header(received.temp_path.as_ref(), received.size_bytes)
            .await
        {
            Ok(metadata) => Some(metadata),
            Err(_) => None,
        },
        Err(_) => None,
    };

    if let Some(metadata) = metadata {
        return PaperRepository::fill_missing_grobid_metadata(&state.posgre, paper_id, &metadata)
            .await?
            .ok_or(PaperSourceHandlerError::NotFound);
    }

    PaperRepository::find_by_id(&state.posgre, paper_id)
        .await?
        .ok_or(PaperSourceHandlerError::NotFound)
}

async fn delete_just_uploaded_pdf(
    state: &AppState,
    object_key: String,
) -> Result<(), PaperSourceHandlerError> {
    state
        .media_object_store
        .delete_object(DeleteMediaObjectInput {
            bucket: state.config.garage.bucket.clone(),
            object_key,
        })
        .await
        .map_err(|_| PaperSourceHandlerError::ObjectStoreFailed)
}

pub async fn update_bibliographic_metadata(
    State(state): State<AppState>,
    AxumPath((source_kind, source_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<UpdatePaperSourceMetadataRequest>,
) -> Result<Json<UpdatePaperSourceMetadataResponse>, PaperSourceHandlerError> {
    let _user = authenticated_user(&state, &headers).await?;
    if source_kind != "paper" {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    let paper_id =
        Uuid::parse_str(&source_id).map_err(|_| PaperSourceHandlerError::InvalidInput)?;
    let doi = request
        .doi
        .map(normalize_doi)
        .filter(|value| !value.trim().is_empty());
    let title = request
        .title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if doi.is_none() && title.is_none() {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    let paper = PaperRepository::complete_missing_bibliographic_metadata(
        &state.posgre,
        paper_id,
        doi.as_deref(),
        title.as_deref(),
    )
    .await?
    .ok_or(PaperSourceHandlerError::NotFound)?;

    Ok(Json(UpdatePaperSourceMetadataResponse {
        source_kind: "paper".to_string(),
        source_id: paper.id,
        requires_bibliographic_input: requires_bibliographic_input(
            paper.doi.as_deref(),
            paper.title.as_deref(),
        ),
        doi: paper.doi,
        title: paper.title,
    }))
}

pub async fn extract_occurrences(
    State(state): State<AppState>,
    AxumPath((source_kind, source_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ExtractPaperSourceOccurrencesResponse>, PaperSourceHandlerError> {
    let _user = authenticated_user(&state, &headers).await?;
    if source_kind != "paper" {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    let paper_id =
        Uuid::parse_str(&source_id).map_err(|_| PaperSourceHandlerError::InvalidInput)?;
    let paper = PaperRepository::find_by_id(&state.posgre, paper_id)
        .await?
        .ok_or(PaperSourceHandlerError::NotFound)?;

    // Extraction never changes status. In particular, a registered paper remains
    // registered while being reprocessed, even if this extraction later fails.
    let source = SourceObjectRow {
        bucket: paper.bucket,
        object_key: paper.object_key,
        size_bytes: paper.size_bytes,
        sha256: paper.sha256,
    };
    let temporary_path =
        download_verified_pdf(&source, state.media_object_store.as_ref()).await?;

    let extractor = LlamaPaperOccurrenceExtractor;
    let result = extractor
        .extract(temporary_path.as_ref())
        .await
        .map_err(map_extraction_error)?;

    let occurrences = result
        .occurrences
        .into_iter()
        .filter_map(|candidate| {
            let scientific_name = candidate.scientific_name.trim().to_string();
            if scientific_name.is_empty() {
                return None;
            }
            Some(PaperSourceOccurrenceCandidate {
                scientific_name,
                locality: candidate
                    .locality
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
            })
        })
        .collect();

    Ok(Json(ExtractPaperSourceOccurrencesResponse {
        source_kind: "paper".to_string(),
        source_id: paper_id,
        occurrences,
    }))
}

fn response_from_paper(row: PaperMetadata, duplicate: bool) -> ReceivePaperSourceResponse {
    let requires_bibliographic_input =
        requires_bibliographic_input(row.doi.as_deref(), row.title.as_deref());
    let message = if duplicate {
        "this paper already has registered occurrence data; confirm before reprocessing"
    } else {
        "paper PDF is stored and ready for occurrence extraction"
    };

    ReceivePaperSourceResponse {
        duplicate,
        source_kind: "paper".to_string(),
        source_id: row.id,
        status: row.status,
        original_filename: row.original_filename,
        content_type: row.content_type,
        size_bytes: row.size_bytes,
        sha256: row.sha256,
        doi: row.doi,
        title: row.title,
        requires_bibliographic_input,
        authors: row.authors,
        publication_year: row.publication_year,
        journal: row.journal,
        volume: row.volume,
        issue: row.issue,
        pages: row.pages,
        article_number: row.article_number,
        message: message.to_string(),
    }
}

async fn download_verified_pdf(
    source: &SourceObjectRow,
    store: &dyn MediaObjectStore,
) -> Result<TempPath, PaperSourceHandlerError> {
    let expected_size = u64::try_from(source.size_bytes)
        .ok()
        .filter(|size| *size > 0 && *size <= PAPER_PDF_FILE_SIZE_LIMIT_BYTES)
        .ok_or(PaperSourceHandlerError::InvalidInput)?;
    let expected_sha256 = source.sha256.trim().to_ascii_lowercase();
    if expected_sha256.len() != 64
        || !expected_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    let mut stream = store
        .get_object(GetMediaObjectInput {
            bucket: source.bucket.clone(),
            object_key: source.object_key.clone(),
        })
        .await
        .map_err(|_| PaperSourceHandlerError::ObjectStoreFailed)?;

    let temporary_path = tempfile::Builder::new()
        .prefix("paper-extraction-")
        .suffix(".pdf")
        .tempfile()
        .map_err(PaperSourceHandlerError::FileSystem)?
        .into_temp_path();
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&temporary_path)
        .await
        .map_err(PaperSourceHandlerError::FileSystem)?;
    let mut hasher = Sha256::new();
    let mut size_bytes = 0_u64;
    let mut signature = Vec::with_capacity(PDF_SIGNATURE.len());

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| PaperSourceHandlerError::ObjectStoreFailed)?;
        size_bytes = size_bytes
            .checked_add(chunk.len() as u64)
            .ok_or(PaperSourceHandlerError::InvalidInput)?;
        if size_bytes > expected_size || size_bytes > PAPER_PDF_FILE_SIZE_LIMIT_BYTES {
            return Err(PaperSourceHandlerError::InvalidInput);
        }
        if signature.len() < PDF_SIGNATURE.len() {
            let remaining = PDF_SIGNATURE.len() - signature.len();
            signature.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        hasher.update(&chunk);
        output
            .write_all(&chunk)
            .await
            .map_err(PaperSourceHandlerError::FileSystem)?;
    }
    output
        .flush()
        .await
        .map_err(PaperSourceHandlerError::FileSystem)?;
    drop(output);

    if size_bytes != expected_size
        || signature.as_slice() != PDF_SIGNATURE
        || hex::encode(hasher.finalize()) != expected_sha256
    {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    Ok(temporary_path)
}

async fn receive_to_temporary_file(
    field: &mut Field<'_>,
) -> Result<ReceivedPdf, PaperSourceHandlerError> {
    let temp_path = tempfile::Builder::new()
        .prefix("paper-upload-")
        .suffix(".pdf")
        .tempfile()
        .map_err(PaperSourceHandlerError::FileSystem)?
        .into_temp_path();
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&temp_path)
        .await
        .map_err(PaperSourceHandlerError::FileSystem)?;
    let mut hasher = Sha256::new();
    let mut size_bytes = 0_u64;
    let mut signature = Vec::with_capacity(PDF_SIGNATURE.len());

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| PaperSourceHandlerError::InvalidInput)?
    {
        size_bytes = size_bytes
            .checked_add(chunk.len() as u64)
            .ok_or(PaperSourceHandlerError::PayloadTooLarge)?;
        if size_bytes > PAPER_PDF_FILE_SIZE_LIMIT_BYTES {
            return Err(PaperSourceHandlerError::PayloadTooLarge);
        }
        if signature.len() < PDF_SIGNATURE.len() {
            let remaining = PDF_SIGNATURE.len() - signature.len();
            signature.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        }
        hasher.update(&chunk);
        output
            .write_all(&chunk)
            .await
            .map_err(PaperSourceHandlerError::FileSystem)?;
    }
    output
        .flush()
        .await
        .map_err(PaperSourceHandlerError::FileSystem)?;
    drop(output);

    if size_bytes == 0 {
        return Err(PaperSourceHandlerError::InvalidInput);
    }
    if signature.as_slice() != PDF_SIGNATURE {
        return Err(PaperSourceHandlerError::UnsupportedMediaType);
    }

    Ok(ReceivedPdf {
        temp_path,
        size_bytes,
        sha256: hex::encode(hasher.finalize()),
    })
}

fn reject_oversized_request_by_content_length(
    headers: &HeaderMap,
) -> Result<(), PaperSourceHandlerError> {
    let Some(value) = headers.get(CONTENT_LENGTH) else {
        return Ok(());
    };
    let value = value
        .to_str()
        .map_err(|_| PaperSourceHandlerError::InvalidInput)?;
    let length = value
        .parse::<u64>()
        .map_err(|_| PaperSourceHandlerError::InvalidInput)?;
    if length > PAPER_SOURCE_PDF_REQUEST_BODY_LIMIT_BYTES as u64 {
        return Err(PaperSourceHandlerError::PayloadTooLarge);
    }
    Ok(())
}

fn validate_filename(filename: Option<&str>) -> Result<(), PaperSourceHandlerError> {
    let filename = filename.ok_or(PaperSourceHandlerError::InvalidInput)?;
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .ok_or(PaperSourceHandlerError::UnsupportedMediaType)?;
    if !extension.eq_ignore_ascii_case("pdf") {
        return Err(PaperSourceHandlerError::UnsupportedMediaType);
    }
    Ok(())
}

async fn authenticated_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<CurrentUserOutput, PaperSourceHandlerError> {
    let token = extract_session_token(headers)?;
    AuthService::current_user(&state.posgre, token)
        .await
        .map_err(Into::into)
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, PaperSourceHandlerError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(PaperSourceHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| PaperSourceHandlerError::InvalidSession)?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            if token.trim().is_empty() {
                return Err(PaperSourceHandlerError::InvalidSession);
            }
            return Ok(token.to_string());
        }
    }
    Err(PaperSourceHandlerError::InvalidSession)
}

fn requires_bibliographic_input(doi: Option<&str>, title: Option<&str>) -> bool {
    !doi.is_some_and(|value| !value.trim().is_empty())
        && !title.is_some_and(|value| !value.trim().is_empty())
}

fn map_extraction_error(_error: PaperLlmExtractionError) -> PaperSourceHandlerError {
    PaperSourceHandlerError::ExtractionFailed
}
