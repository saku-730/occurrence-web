use std::path::Path;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::features::media::service::{
    GetMediaObjectInput, MediaObjectStore, MediaServiceError,
};

use super::{
    llama::{
        OccurrenceExtractionResult, PaperLlmExtractionError, extract_occurrences_from_pdf,
    },
    service::PAPER_PDF_FILE_SIZE_LIMIT_BYTES,
};

const PDF_SIGNATURE: &[u8] = b"%PDF-";

#[derive(Debug)]
pub enum PaperOccurrenceExtractionError {
    NotFound,
    ObjectStoreFailed,
    InvalidStoredPdf,
    FileSystem(std::io::Error),
    Database(sqlx::Error),
    Extractor(PaperLlmExtractionError),
}

impl From<sqlx::Error> for PaperOccurrenceExtractionError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<std::io::Error> for PaperOccurrenceExtractionError {
    fn from(error: std::io::Error) -> Self {
        Self::FileSystem(error)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ExtractionSourceRow {
    bucket: String,
    object_key: String,
    size_bytes: i64,
    sha256: String,
}

#[derive(Debug)]
pub struct ExtractPaperOccurrencesOutput {
    pub import_id: Uuid,
    pub result: OccurrenceExtractionResult,
}

#[async_trait::async_trait]
pub trait PaperOccurrenceExtractor: Send + Sync {
    async fn extract(
        &self,
        pdf_path: &Path,
    ) -> Result<OccurrenceExtractionResult, PaperLlmExtractionError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LlamaPaperOccurrenceExtractor;

#[async_trait::async_trait]
impl PaperOccurrenceExtractor for LlamaPaperOccurrenceExtractor {
    async fn extract(
        &self,
        pdf_path: &Path,
    ) -> Result<OccurrenceExtractionResult, PaperLlmExtractionError> {
        extract_occurrences_from_pdf(pdf_path).await
    }
}

pub struct PaperOccurrenceExtractionService;

impl PaperOccurrenceExtractionService {
    pub async fn extract<S, E>(
        import_id: Uuid,
        uploaded_by: Uuid,
        store: &S,
        extractor: &E,
        db: &PgPool,
    ) -> Result<ExtractPaperOccurrencesOutput, PaperOccurrenceExtractionError>
    where
        S: MediaObjectStore + ?Sized,
        E: PaperOccurrenceExtractor + ?Sized,
    {
        // Ownership and state are checked in the same UPDATE so another request
        // cannot start extraction for the same staged import concurrently.
        let source = sqlx::query_as::<_, ExtractionSourceRow>(
            r#"
            UPDATE paper_imports
            SET status = 'extracting_occurrences', updated_at = now()
            WHERE id = $1
              AND uploaded_by = $2
              AND status = 'staged'
            RETURNING bucket, object_key, size_bytes, sha256
            "#,
        )
        .bind(import_id)
        .bind(uploaded_by)
        .fetch_optional(db)
        .await?
        .ok_or(PaperOccurrenceExtractionError::NotFound)?;

        let extraction_result = Self::download_and_extract(&source, store, extractor).await;

        match extraction_result {
            Ok(result) => {
                let updated = sqlx::query(
                    r#"
                    UPDATE paper_imports
                    SET status = 'reviewing', updated_at = now()
                    WHERE id = $1
                      AND uploaded_by = $2
                      AND status = 'extracting_occurrences'
                    "#,
                )
                .bind(import_id)
                .bind(uploaded_by)
                .execute(db)
                .await?;

                if updated.rows_affected() != 1 {
                    return Err(PaperOccurrenceExtractionError::NotFound);
                }

                Ok(ExtractPaperOccurrencesOutput { import_id, result })
            }
            Err(error) => {
                // A handled extraction failure returns the import to staged so
                // the same endpoint can be retried and the existing cancel path
                // remains valid. A process crash is intentionally different and
                // may leave extracting_occurrences for later reconciliation.
                sqlx::query(
                    r#"
                    UPDATE paper_imports
                    SET status = 'staged', updated_at = now()
                    WHERE id = $1
                      AND uploaded_by = $2
                      AND status = 'extracting_occurrences'
                    "#,
                )
                .bind(import_id)
                .bind(uploaded_by)
                .execute(db)
                .await?;

                Err(error)
            }
        }
    }

    async fn download_and_extract<S, E>(
        source: &ExtractionSourceRow,
        store: &S,
        extractor: &E,
    ) -> Result<OccurrenceExtractionResult, PaperOccurrenceExtractionError>
    where
        S: MediaObjectStore + ?Sized,
        E: PaperOccurrenceExtractor + ?Sized,
    {
        let expected_size = u64::try_from(source.size_bytes)
            .ok()
            .filter(|size| *size > 0 && *size <= PAPER_PDF_FILE_SIZE_LIMIT_BYTES)
            .ok_or(PaperOccurrenceExtractionError::InvalidStoredPdf)?;
        let expected_sha256 = source.sha256.trim().to_ascii_lowercase();
        if expected_sha256.len() != 64
            || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(PaperOccurrenceExtractionError::InvalidStoredPdf);
        }

        let mut stream = store
            .get_object(GetMediaObjectInput {
                bucket: source.bucket.clone(),
                object_key: source.object_key.clone(),
            })
            .await
            .map_err(map_object_store_error)?;

        let temporary_path = tempfile::Builder::new()
            .prefix("paper-extraction-")
            .suffix(".pdf")
            .tempfile()?
            .into_temp_path();
        let mut output = tokio::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&temporary_path)
            .await?;
        let mut hasher = Sha256::new();
        let mut size_bytes = 0_u64;
        let mut signature = Vec::with_capacity(PDF_SIGNATURE.len());

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_object_store_error)?;
            size_bytes = size_bytes
                .checked_add(chunk.len() as u64)
                .ok_or(PaperOccurrenceExtractionError::InvalidStoredPdf)?;
            if size_bytes > PAPER_PDF_FILE_SIZE_LIMIT_BYTES || size_bytes > expected_size {
                return Err(PaperOccurrenceExtractionError::InvalidStoredPdf);
            }

            if signature.len() < PDF_SIGNATURE.len() {
                let remaining = PDF_SIGNATURE.len() - signature.len();
                signature.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            }

            hasher.update(&chunk);
            output.write_all(&chunk).await?;
        }
        output.flush().await?;
        drop(output);

        if size_bytes != expected_size
            || signature.as_slice() != PDF_SIGNATURE
            || hex::encode(hasher.finalize()) != expected_sha256
        {
            return Err(PaperOccurrenceExtractionError::InvalidStoredPdf);
        }

        extractor
            .extract(temporary_path.as_ref())
            .await
            .map_err(PaperOccurrenceExtractionError::Extractor)
    }
}

fn map_object_store_error(_error: MediaServiceError) -> PaperOccurrenceExtractionError {
    PaperOccurrenceExtractionError::ObjectStoreFailed
}
