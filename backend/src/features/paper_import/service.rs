use std::path::PathBuf;

use sqlx::PgPool;
use uuid::Uuid;

use crate::features::media::service::{
    DeleteMediaObjectInput, MediaObjectStore, PutMediaObjectInput,
};

use super::{
    grobid::{GrobidClient, GrobidError, GrobidPaperMetadata},
    repository::{InsertPaperMetadata, PaperMetadata, PaperRepository},
};

#[derive(Debug)]
pub enum PaperImportServiceError {
    InvalidInput,
    ObjectStoreFailed,
    Database(sqlx::Error),
    Grobid(GrobidError),
    ConflictResolutionFailed,
}

impl From<sqlx::Error> for PaperImportServiceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Clone)]
pub struct ImportPaperPdfInput {
    pub bucket: String,
    pub uploaded_by: Uuid,
    pub original_filename: Option<String>,
    pub content_type: String,
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportPaperPdfStatus {
    Imported,
    AlreadyImported,
}

#[derive(Debug, Clone)]
pub struct ImportPaperPdfOutput {
    pub status: ImportPaperPdfStatus,
    pub paper_id: Uuid,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub original_filename: Option<String>,
    pub sha256: String,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub authors: Option<String>,
    pub publication_year: Option<i32>,
    pub journal: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub article_number: Option<String>,
}

pub struct PaperImportService;

impl PaperImportService {
    pub async fn import_pdf<S>(
        input: ImportPaperPdfInput,
        store: &S,
        db: &PgPool,
    ) -> Result<ImportPaperPdfOutput, PaperImportServiceError>
    where
        S: MediaObjectStore + ?Sized,
    {
        let bucket = input.bucket.trim();
        let content_type = input.content_type.trim();
        let sha256 = input.payload_sha256.trim().to_ascii_lowercase();

        if bucket.is_empty()
            || content_type != "application/pdf"
            || input.size_bytes == 0
            || !is_valid_sha256_hex(&sha256)
        {
            return Err(PaperImportServiceError::InvalidInput);
        }

        // 同一PDFが既に確定済みなら、GarageにもGROBIDにも送らずここで終了する。
        if let Some(existing) = PaperRepository::find_by_sha256(db, &sha256).await? {
            return Ok(output_from_metadata(
                ImportPaperPdfStatus::AlreadyImported,
                existing,
            ));
        }

        // 設定不備をGarage PUTより先に検出する。
        let grobid = GrobidClient::from_env().map_err(PaperImportServiceError::Grobid)?;

        let paper_id = Uuid::new_v4();
        let object_key = format!("papers/{paper_id}/original.pdf");

        store
            .put_object(PutMediaObjectInput {
                bucket: bucket.to_string(),
                object_key: object_key.clone(),
                content_type: content_type.to_string(),
                file_path: input.file_path.clone(),
                size_bytes: input.size_bytes,
                payload_sha256: sha256.clone(),
            })
            .await
            .map_err(|_| PaperImportServiceError::ObjectStoreFailed)?;

        // Garage保存後に同じ一時PDFをGROBIDへstreamする。
        // GROBID失敗時はpaperをimport済みにしないため、Garage objectを削除して巻き戻す。
        let grobid_metadata = match grobid
            .extract_header(&input.file_path, input.size_bytes)
            .await
        {
            Ok(metadata) => metadata,
            Err(error) => {
                rollback_object(store, bucket, &object_key).await?;
                return Err(PaperImportServiceError::Grobid(error));
            }
        };

        let insert_result = PaperRepository::insert_if_sha256_absent(
            db,
            InsertPaperMetadata {
                id: paper_id,
                bucket,
                object_key: &object_key,
                content_type,
                size_bytes: input.size_bytes as i64,
                original_filename: input.original_filename.as_deref(),
                sha256: &sha256,
                doi: grobid_metadata.doi.as_deref(),
                title: grobid_metadata.title.as_deref(),
                authors: grobid_metadata.authors.as_deref(),
                publication_year: grobid_metadata.publication_year,
                journal: grobid_metadata.journal.as_deref(),
                volume: grobid_metadata.volume.as_deref(),
                issue: grobid_metadata.issue.as_deref(),
                pages: grobid_metadata.pages.as_deref(),
                article_number: grobid_metadata.article_number.as_deref(),
                uploaded_by: input.uploaded_by,
            },
        )
        .await;

        let inserted = match insert_result {
            Ok(inserted) => inserted,
            Err(database_error) => {
                // Garageだけに孤立objectを残さないよう、DB失敗時はPUTを巻き戻す。
                rollback_object(store, bucket, &object_key).await?;
                return Err(PaperImportServiceError::Database(database_error));
            }
        };

        if inserted {
            return Ok(output_from_new_import(
                paper_id,
                bucket,
                object_key,
                content_type,
                input.size_bytes as i64,
                input.original_filename,
                sha256,
                grobid_metadata,
            ));
        }

        // 事前確認からINSERTまでの間に同じSHA-256が別requestで確定した場合。
        // 自分のGarage objectは不要なので削除し、先に確定したpaperを返す。
        rollback_object(store, bucket, &object_key).await?;

        let existing = PaperRepository::find_by_sha256(db, &sha256)
            .await?
            .ok_or(PaperImportServiceError::ConflictResolutionFailed)?;

        Ok(output_from_metadata(
            ImportPaperPdfStatus::AlreadyImported,
            existing,
        ))
    }
}

async fn rollback_object<S>(
    store: &S,
    bucket: &str,
    object_key: &str,
) -> Result<(), PaperImportServiceError>
where
    S: MediaObjectStore + ?Sized,
{
    store
        .delete_object(DeleteMediaObjectInput {
            bucket: bucket.to_string(),
            object_key: object_key.to_string(),
        })
        .await
        .map_err(|_| PaperImportServiceError::ObjectStoreFailed)
}

fn output_from_new_import(
    paper_id: Uuid,
    bucket: &str,
    object_key: String,
    content_type: &str,
    size_bytes: i64,
    original_filename: Option<String>,
    sha256: String,
    metadata: GrobidPaperMetadata,
) -> ImportPaperPdfOutput {
    ImportPaperPdfOutput {
        status: ImportPaperPdfStatus::Imported,
        paper_id,
        bucket: bucket.to_string(),
        object_key,
        content_type: content_type.to_string(),
        size_bytes,
        original_filename,
        sha256,
        doi: metadata.doi,
        title: metadata.title,
        authors: metadata.authors,
        publication_year: metadata.publication_year,
        journal: metadata.journal,
        volume: metadata.volume,
        issue: metadata.issue,
        pages: metadata.pages,
        article_number: metadata.article_number,
    }
}

fn output_from_metadata(
    status: ImportPaperPdfStatus,
    metadata: PaperMetadata,
) -> ImportPaperPdfOutput {
    ImportPaperPdfOutput {
        status,
        paper_id: metadata.id,
        bucket: metadata.bucket,
        object_key: metadata.object_key,
        content_type: metadata.content_type,
        size_bytes: metadata.size_bytes,
        original_filename: metadata.original_filename,
        sha256: metadata.sha256,
        doi: metadata.doi,
        title: metadata.title,
        authors: metadata.authors,
        publication_year: metadata.publication_year,
        journal: metadata.journal,
        volume: metadata.volume,
        issue: metadata.issue,
        pages: metadata.pages,
        article_number: metadata.article_number,
    }
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
