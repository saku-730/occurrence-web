use std::path::PathBuf;

use sqlx::PgPool;
use uuid::Uuid;

use crate::features::media::service::{
    DeleteMediaObjectInput, MediaObjectStore, PutMediaObjectInput,
};

use super::repository::{InsertPaperMetadata, PaperMetadata, PaperRepository};

#[derive(Debug)]
pub enum PaperImportServiceError {
    InvalidInput,
    ObjectStoreFailed,
    Database(sqlx::Error),
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

        // 通常の重複はGarageへ送る前にここで終了する。
        if let Some(existing) = PaperRepository::find_by_sha256(db, &sha256).await? {
            return Ok(output_from_metadata(
                ImportPaperPdfStatus::AlreadyImported,
                existing,
            ));
        }

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
                uploaded_by: input.uploaded_by,
            },
        )
        .await;

        let inserted = match insert_result {
            Ok(inserted) => inserted,
            Err(database_error) => {
                // Garageだけに孤立objectを残さないよう、DB失敗時はPUTを巻き戻す。
                store
                    .delete_object(DeleteMediaObjectInput {
                        bucket: bucket.to_string(),
                        object_key: object_key.clone(),
                    })
                    .await
                    .map_err(|_| PaperImportServiceError::ObjectStoreFailed)?;
                return Err(PaperImportServiceError::Database(database_error));
            }
        };

        if inserted {
            return Ok(ImportPaperPdfOutput {
                status: ImportPaperPdfStatus::Imported,
                paper_id,
                bucket: bucket.to_string(),
                object_key,
                content_type: content_type.to_string(),
                size_bytes: input.size_bytes as i64,
                original_filename: input.original_filename,
                sha256,
            });
        }

        // 事前確認後に同じSHA-256が同時登録された場合。
        // 自分が作ったGarage objectを削除し、先に確定したpaperを返す。
        store
            .delete_object(DeleteMediaObjectInput {
                bucket: bucket.to_string(),
                object_key,
            })
            .await
            .map_err(|_| PaperImportServiceError::ObjectStoreFailed)?;

        let existing = PaperRepository::find_by_sha256(db, &sha256)
            .await?
            .ok_or(PaperImportServiceError::ConflictResolutionFailed)?;

        Ok(output_from_metadata(
            ImportPaperPdfStatus::AlreadyImported,
            existing,
        ))
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
    }
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
