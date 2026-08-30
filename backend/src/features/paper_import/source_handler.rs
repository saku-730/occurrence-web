use std::path::{Path, PathBuf};

use axum::{
    Json,
    extract::{Multipart, Path as AxumPath, State, multipart::Field},
    http::{HeaderMap, StatusCode, header::{CONTENT_LENGTH, COOKIE}},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tempfile::TempPath;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        media::service::{
            DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectStore, PutMediaObjectInput,
        },
    },
    state::AppState,
};

use super::{
    extraction::{LlamaPaperOccurrenceExtractor, PaperOccurrenceExtractor},
    grobid::{GrobidClient, GrobidError, normalize_doi},
    llama::PaperLlmExtractionError,
    repository::{PaperMetadata, PaperRepository},
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

impl IntoResponse for PaperSourceHandlerError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            Self::InvalidSession => (StatusCode::UNAUTHORIZED, "invalid_session", "Invalid session"),
            Self::InvalidInput => (StatusCode::BAD_REQUEST, "invalid_paper_source", "Invalid paper source request"),
            Self::NotFound => (StatusCode::NOT_FOUND, "paper_source_not_found", "Paper source not found"),
            Self::UnsupportedMediaType => (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type", "Only PDF files are accepted"),
            Self::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "PDF file exceeds the 100MB limit"),
            Self::ObjectStoreFailed => (StatusCode::BAD_GATEWAY, "object_store_error", "Failed to read or store the paper PDF"),
            Self::GrobidFailed => (StatusCode::BAD_GATEWAY, "grobid_error", "Failed to extract paper metadata with GROBID"),
            Self::ExtractionFailed => (StatusCode::BAD_GATEWAY, "occurrence_extraction_error", "Failed to extract occurrences from the paper"),
            Self::Database(_) | Self::FileSystem(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error", "Internal server error"),
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
    pub duplicate: bool,
    pub source_kind: String,
    pub source_id: Uuid,
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

#[derive(Debug, sqlx::FromRow)]
struct ExistingImportRow {
    id: Uuid,
    bucket: String,
    object_key: String,
    original_filename: Option<String>,
    content_type: String,
    size_bytes: i64,
    sha256: String,
    doi: Option<String>,
    title: Option<String>,
    authors: Option<String>,
    publication_year: Option<i32>,
    journal: Option<String>,
    volume: Option<String>,
    issue: Option<String>,
    pages: Option<String>,
    article_number: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct SourceObjectRow {
    bucket: String,
    object_key: String,
    size_bytes: i64,
    sha256: String,
}

#[derive(Debug, sqlx::FromRow)]
struct MetadataRow {
    doi: Option<String>,
    title: Option<String>,
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
        if !field_content_type.trim().eq_ignore_ascii_case("application/pdf") {
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
    // A formally registered paper is reusable by SHA-256. Do not create another
    // Garage object or PostgreSQL row; the browser decides whether to continue.
    if let Some(existing) = PaperRepository::find_by_sha256(&state.posgre, &received.sha256).await? {
        return Ok((StatusCode::OK, Json(response_from_paper(existing, true))));
    }

    // Before formal registration, only reuse the current user's own source row.
    // This avoids exposing another user's uncommitted import by UUID.
    if let Some(existing) = find_existing_import(
        &state.posgre,
        uploaded_by,
        &received.sha256,
    )
    .await?
    {
        return Ok((StatusCode::OK, Json(response_from_import(existing, true))));
    }

    let grobid = GrobidClient::from_env().map_err(|_| PaperSourceHandlerError::GrobidFailed)?;
    let metadata = grobid
        .extract_header(&received.temp_path, received.size_bytes)
        .await
        .map_err(|_| PaperSourceHandlerError::GrobidFailed)?;

    // Recheck after GROBID because a concurrent request may have completed while
    // metadata extraction was running.
    if let Some(existing) = PaperRepository::find_by_sha256(&state.posgre, &received.sha256).await? {
        return Ok((StatusCode::OK, Json(response_from_paper(existing, true))));
    }
    if let Some(existing) = find_existing_import(
        &state.posgre,
        uploaded_by,
        &received.sha256,
    )
    .await?
    {
        return Ok((StatusCode::OK, Json(response_from_import(existing, true))));
    }

    let import_id = Uuid::new_v4();
    let reserved_paper_id = Uuid::new_v4();
    let object_key = format!("papers/{reserved_paper_id}/original.pdf");
    let size_bytes = i64::try_from(received.size_bytes)
        .map_err(|_| PaperSourceHandlerError::InvalidInput)?;

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

    // status remains only as a legacy schema value. New source/extraction flows do
    // not read it and do not use it as a state machine.
    let insert = sqlx::query(
        r#"
        INSERT INTO paper_imports (
            id, reserved_paper_id, bucket, object_key, content_type,
            size_bytes, original_filename, sha256,
            doi, title, authors, publication_year, journal,
            volume, issue, pages, article_number,
            uploaded_by, status
        )
        VALUES (
            $1, $2, $3, $4, $5,
            $6, $7, $8,
            $9, $10, $11, $12, $13,
            $14, $15, $16, $17,
            $18, 'staged'
        )
        "#,
    )
    .bind(import_id)
    .bind(reserved_paper_id)
    .bind(&state.config.garage.bucket)
    .bind(&object_key)
    .bind(&content_type)
    .bind(size_bytes)
    .bind(original_filename.as_deref())
    .bind(&received.sha256)
    .bind(metadata.doi.as_deref())
    .bind(metadata.title.as_deref())
    .bind(metadata.authors.as_deref())
    .bind(metadata.publication_year)
    .bind(metadata.journal.as_deref())
    .bind(metadata.volume.as_deref())
    .bind(metadata.issue.as_deref())
    .bind(metadata.pages.as_deref())
    .bind(metadata.article_number.as_deref())
    .bind(uploaded_by)
    .execute(&state.posgre)
    .await;

    if let Err(error) = insert {
        let cleanup = state
            .media_object_store
            .delete_object(DeleteMediaObjectInput {
                bucket: state.config.garage.bucket.clone(),
                object_key,
            })
            .await;
        if cleanup.is_err() {
            return Err(PaperSourceHandlerError::ObjectStoreFailed);
        }
        return Err(PaperSourceHandlerError::Database(error));
    }

    Ok((
        StatusCode::CREATED,
        Json(ReceivePaperSourceResponse {
            duplicate: false,
            source_kind: "import".to_string(),
            source_id: import_id,
            original_filename,
            content_type,
            size_bytes,
            sha256: received.sha256.clone(),
            doi: metadata.doi,
            title: metadata.title,
            requires_bibliographic_input: requires_bibliographic_input(
                metadata.doi.as_deref(),
                metadata.title.as_deref(),
            ),
            authors: metadata.authors,
            publication_year: metadata.publication_year,
            journal: metadata.journal,
            volume: metadata.volume,
            issue: metadata.issue,
            pages: metadata.pages,
            article_number: metadata.article_number,
            message: "paper PDF stored for import".to_string(),
        }),
    ))
}

pub async fn update_bibliographic_metadata(
    State(state): State<AppState>,
    AxumPath((source_kind, source_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<UpdatePaperSourceMetadataRequest>,
) -> Result<Json<UpdatePaperSourceMetadataResponse>, PaperSourceHandlerError> {
    let user = authenticated_user(&state, &headers).await?;
    let source_id = Uuid::parse_str(&source_id).map_err(|_| PaperSourceHandlerError::InvalidInput)?;
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

    let row = match source_kind.as_str() {
        "import" => {
            sqlx::query_as::<_, MetadataRow>(
                r#"
                UPDATE paper_imports
                SET doi = CASE WHEN doi IS NULL OR BTRIM(doi) = '' THEN $3 ELSE doi END,
                    title = CASE WHEN title IS NULL OR BTRIM(title) = '' THEN $4 ELSE title END,
                    updated_at = now()
                WHERE id = $1 AND uploaded_by = $2
                RETURNING doi, title
                "#,
            )
            .bind(source_id)
            .bind(user.user_id)
            .bind(doi.as_deref())
            .bind(title.as_deref())
            .fetch_optional(&state.posgre)
            .await?
            .ok_or(PaperSourceHandlerError::NotFound)?
        }
        "paper" => {
            let paper = PaperRepository::complete_missing_bibliographic_metadata(
                &state.posgre,
                source_id,
                doi.as_deref(),
                title.as_deref(),
            )
            .await?
            .ok_or(PaperSourceHandlerError::NotFound)?;
            MetadataRow { doi: paper.doi, title: paper.title }
        }
        _ => return Err(PaperSourceHandlerError::InvalidInput),
    };

    Ok(Json(UpdatePaperSourceMetadataResponse {
        source_kind,
        source_id,
        requires_bibliographic_input: requires_bibliographic_input(
            row.doi.as_deref(),
            row.title.as_deref(),
        ),
        doi: row.doi,
        title: row.title,
    }))
}

pub async fn extract_occurrences(
    State(state): State<AppState>,
    AxumPath((source_kind, source_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ExtractPaperSourceOccurrencesResponse>, PaperSourceHandlerError> {
    let user = authenticated_user(&state, &headers).await?;
    let source_id = Uuid::parse_str(&source_id).map_err(|_| PaperSourceHandlerError::InvalidInput)?;

    let source = match source_kind.as_str() {
        "import" => sqlx::query_as::<_, SourceObjectRow>(
            r#"
            SELECT bucket, object_key, size_bytes, sha256
            FROM paper_imports
            WHERE id = $1 AND uploaded_by = $2
            "#,
        )
        .bind(source_id)
        .bind(user.user_id)
        .fetch_optional(&state.posgre)
        .await?
        .ok_or(PaperSourceHandlerError::NotFound)?,
        "paper" => sqlx::query_as::<_, SourceObjectRow>(
            r#"
            SELECT bucket, object_key, size_bytes, sha256
            FROM papers
            WHERE id = $1
            "#,
        )
        .bind(source_id)
        .fetch_optional(&state.posgre)
        .await?
        .ok_or(PaperSourceHandlerError::NotFound)?,
        _ => return Err(PaperSourceHandlerError::InvalidInput),
    };

    let temporary_path = download_verified_pdf(
        &source,
        state.media_object_store.as_ref(),
    )
    .await?;

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
        source_kind,
        source_id,
        occurrences,
    }))
}

async fn find_existing_import(
    db: &PgPool,
    uploaded_by: Uuid,
    sha256: &str,
) -> Result<Option<ExistingImportRow>, sqlx::Error> {
    sqlx::query_as::<_, ExistingImportRow>(
        r#"
        SELECT id, bucket, object_key, original_filename, content_type,
               size_bytes, sha256, doi, title, authors, publication_year,
               journal, volume, issue, pages, article_number
        FROM paper_imports
        WHERE uploaded_by = $1
          AND sha256 = $2
          AND status <> 'cancelling'
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(uploaded_by)
    .bind(sha256)
    .fetch_optional(db)
    .await
}

fn response_from_import(row: ExistingImportRow, duplicate: bool) -> ReceivePaperSourceResponse {
    ReceivePaperSourceResponse {
        duplicate,
        source_kind: "import".to_string(),
        source_id: row.id,
        original_filename: row.original_filename,
        content_type: row.content_type,
        size_bytes: row.size_bytes,
        sha256: row.sha256,
        requires_bibliographic_input: requires_bibliographic_input(
            row.doi.as_deref(),
            row.title.as_deref(),
        ),
        doi: row.doi,
        title: row.title,
        authors: row.authors,
        publication_year: row.publication_year,
        journal: row.journal,
        volume: row.volume,
        issue: row.issue,
        pages: row.pages,
        article_number: row.article_number,
        message: "identical PDF was uploaded before; existing source reused".to_string(),
    }
}

fn response_from_paper(row: PaperMetadata, duplicate: bool) -> ReceivePaperSourceResponse {
    ReceivePaperSourceResponse {
        duplicate,
        source_kind: "paper".to_string(),
        source_id: row.id,
        original_filename: row.original_filename,
        content_type: row.content_type,
        size_bytes: row.size_bytes,
        sha256: row.sha256,
        requires_bibliographic_input: requires_bibliographic_input(
            row.doi.as_deref(),
            row.title.as_deref(),
        ),
        doi: row.doi,
        title: row.title,
        authors: row.authors,
        publication_year: row.publication_year,
        journal: row.journal,
        volume: row.volume,
        issue: row.issue,
        pages: row.pages,
        article_number: row.article_number,
        message: "identical PDF was imported before; existing paper reused".to_string(),
    }
}

async fn download_verified_pdf(
    source: &SourceObjectRow,
    store: &(dyn MediaObjectStore),
) -> Result<tempfile::TempPath, PaperSourceHandlerError> {
    let expected_size = u64::try_from(source.size_bytes)
        .ok()
        .filter(|size| *size > 0 && *size <= PAPER_PDF_FILE_SIZE_LIMIT_BYTES)
        .ok_or(PaperSourceHandlerError::InvalidInput)?;
    let expected_sha256 = source.sha256.trim().to_ascii_lowercase();
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
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
        .prefix("paper-source-extraction-")
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
    output.flush().await.map_err(PaperSourceHandlerError::FileSystem)?;
    drop(output);

    if size_bytes != expected_size
        || signature.as_slice() != PDF_SIGNATURE
        || hex::encode(hasher.finalize()) != expected_sha256
    {
        return Err(PaperSourceHandlerError::InvalidInput);
    }

    Ok(temporary_path)
}

async fn receive_to_temporary_file(field: &mut Field<'_>) -> Result<ReceivedPdf, PaperSourceHandlerError> {
    let temp_path = tempfile::Builder::new()
        .prefix("paper-source-upload-")
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
    output.flush().await.map_err(PaperSourceHandlerError::FileSystem)?;
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

fn reject_oversized_request_by_content_length(headers: &HeaderMap) -> Result<(), PaperSourceHandlerError> {
    let Some(value) = headers.get(CONTENT_LENGTH) else {
        return Ok(());
    };
    let value = value.to_str().map_err(|_| PaperSourceHandlerError::InvalidInput)?;
    let length = value.parse::<u64>().map_err(|_| PaperSourceHandlerError::InvalidInput)?;
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
) -> Result<crate::features::auth::dto::UserResponse, PaperSourceHandlerError> {
    let token = extract_session_token(headers)?;
    AuthService::current_user(&state.posgre, token).await.map_err(Into::into)
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
