use std::path::PathBuf;

use sqlx::PgPool;
use uuid::Uuid;

use crate::features::media::service::{
    DeleteMediaObjectInput, MediaObjectStore, PutMediaObjectInput,
};

use super::{
    grobid::{GrobidClient, GrobidError, GrobidPaperMetadata, PaperMetadataExtractor},
    repository::{PaperMetadata, PaperRepository},
    service::PAPER_PDF_FILE_SIZE_LIMIT_BYTES,
};

#[derive(Debug)]
pub enum PaperImportStagingError {
    InvalidInput,
    ObjectStoreFailed,
    Database(sqlx::Error),
    Grobid(GrobidError),
    NotFound,
}

impl From<sqlx::Error> for PaperImportStagingError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Clone)]
pub struct StartPaperImportInput {
    pub bucket: String,
    pub uploaded_by: Uuid,
    pub original_filename: Option<String>,
    pub content_type: String,
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPaperImportStatus {
    Staged,
    MetadataRequired,
    AlreadyImported,
}

#[derive(Debug, Clone)]
pub struct StartPaperImportOutput {
    pub status: StartPaperImportStatus,
    pub import_id: Option<Uuid>,
    pub paper_id: Option<Uuid>,
    pub original_filename: Option<String>,
    pub content_type: String,
    pub size_bytes: i64,
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
    pub requires_bibliographic_input: bool,
}

#[derive(Debug, Clone)]
pub struct CompleteStagedBibliographicMetadataInput {
    pub import_id: Uuid,
    pub uploaded_by: Uuid,
    pub doi: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CompleteStagedBibliographicMetadataOutput {
    pub import_id: Uuid,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub requires_bibliographic_input: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct StagedPaperImportRow {
    id: Uuid,
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
    status: String,
}

#[derive(Debug, sqlx::FromRow)]
struct CompletedMetadataRow {
    id: Uuid,
    doi: Option<String>,
    title: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct CancellingImportRow {
    bucket: String,
    object_key: String,
}

pub struct PaperImportStagingService;

impl PaperImportStagingService {
    pub async fn start<S>(
        input: StartPaperImportInput,
        store: &S,
        db: &PgPool,
    ) -> Result<StartPaperImportOutput, PaperImportStagingError>
    where
        S: MediaObjectStore + ?Sized,
    {
        let input = normalize_start_input(input)?;

        // 正式登録済みのPDFは従来どおりSHA-256で即停止する。
        if let Some(existing) = PaperRepository::find_by_sha256(db, &input.payload_sha256).await? {
            return Ok(output_from_registered_paper(existing));
        }

        // 同じユーザーが同一PDFの仮importを既に持っている場合は再利用し、
        // GROBIDやGarageへの重複処理を行わない。
        if let Some(existing) = find_active_staged_import(
            db,
            input.uploaded_by,
            &input.payload_sha256,
        )
        .await?
        {
            return Ok(output_from_staged_row(existing));
        }

        let grobid = GrobidClient::from_env().map_err(PaperImportStagingError::Grobid)?;
        let metadata = grobid
            .extract_header(&input.file_path, input.size_bytes)
            .await
            .map_err(PaperImportStagingError::Grobid)?;

        // GROBID実行中に別requestが正式登録を完了した可能性があるため再確認する。
        if let Some(existing) = PaperRepository::find_by_sha256(db, &input.payload_sha256).await? {
            return Ok(output_from_registered_paper(existing));
        }

        let import_id = Uuid::new_v4();
        let reserved_paper_id = Uuid::new_v4();
        // Garage上には確認処理のため一時保持するが、papers行はまだ作らない。
        // 将来の確定時にcopy/move不要でそのまま正式paperへ昇格できるよう、
        // object keyだけは予約済みpaper UUIDで最終形を使う。
        let object_key = format!("papers/{reserved_paper_id}/original.pdf");
        let size_bytes = i64::try_from(input.size_bytes)
            .expect("paper size was validated before staging side effects");
        let status = if requires_bibliographic_input(
            metadata.doi.as_deref(),
            metadata.title.as_deref(),
        ) {
            "metadata_required"
        } else {
            "staged"
        };

        store
            .put_object(PutMediaObjectInput {
                bucket: input.bucket.clone(),
                object_key: object_key.clone(),
                content_type: input.content_type.clone(),
                file_path: input.file_path.clone(),
                size_bytes: input.size_bytes,
                payload_sha256: input.payload_sha256.clone(),
            })
            .await
            .map_err(|_| PaperImportStagingError::ObjectStoreFailed)?;

        let insert_result = sqlx::query(
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
                $18, $19
            )
            "#,
        )
        .bind(import_id)
        .bind(reserved_paper_id)
        .bind(&input.bucket)
        .bind(&object_key)
        .bind(&input.content_type)
        .bind(size_bytes)
        .bind(input.original_filename.as_deref())
        .bind(&input.payload_sha256)
        .bind(metadata.doi.as_deref())
        .bind(metadata.title.as_deref())
        .bind(metadata.authors.as_deref())
        .bind(metadata.publication_year)
        .bind(metadata.journal.as_deref())
        .bind(metadata.volume.as_deref())
        .bind(metadata.issue.as_deref())
        .bind(metadata.pages.as_deref())
        .bind(metadata.article_number.as_deref())
        .bind(input.uploaded_by)
        .bind(status)
        .execute(db)
        .await;

        if let Err(error) = insert_result {
            let cleanup = store
                .delete_object(DeleteMediaObjectInput {
                    bucket: input.bucket.clone(),
                    object_key: object_key.clone(),
                })
                .await;
            if cleanup.is_err() {
                return Err(PaperImportStagingError::ObjectStoreFailed);
            }
            return Err(PaperImportStagingError::Database(error));
        }

        Ok(output_from_new_staging(
            import_id,
            input.original_filename,
            input.content_type,
            size_bytes,
            input.payload_sha256,
            metadata,
        ))
    }

    pub async fn complete_bibliographic_metadata(
        input: CompleteStagedBibliographicMetadataInput,
        db: &PgPool,
    ) -> Result<CompleteStagedBibliographicMetadataOutput, PaperImportStagingError> {
        let doi = input
            .doi
            .map(super::grobid::normalize_doi)
            .filter(|value| !value.is_empty());
        let title = input
            .title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if doi.is_none() && title.is_none() {
            return Err(PaperImportStagingError::InvalidInput);
        }

        let row = sqlx::query_as::<_, CompletedMetadataRow>(
            r#"
            UPDATE paper_imports
            SET doi = CASE
                    WHEN doi IS NULL OR BTRIM(doi) = '' THEN $3
                    ELSE doi
                END,
                title = CASE
                    WHEN title IS NULL OR BTRIM(title) = '' THEN $4
                    ELSE title
                END,
                status = CASE
                    WHEN COALESCE(NULLIF(BTRIM(doi), ''), NULLIF(BTRIM($3::text), '')) IS NOT NULL
                      OR COALESCE(NULLIF(BTRIM(title), ''), NULLIF(BTRIM($4::text), '')) IS NOT NULL
                    THEN 'staged'
                    ELSE 'metadata_required'
                END,
                updated_at = now()
            WHERE id = $1
              AND uploaded_by = $2
              AND status IN ('metadata_required', 'staged')
            RETURNING id, doi, title
            "#,
        )
        .bind(input.import_id)
        .bind(input.uploaded_by)
        .bind(doi.as_deref())
        .bind(title.as_deref())
        .fetch_optional(db)
        .await?
        .ok_or(PaperImportStagingError::NotFound)?;

        Ok(CompleteStagedBibliographicMetadataOutput {
            import_id: row.id,
            requires_bibliographic_input: requires_bibliographic_input(
                row.doi.as_deref(),
                row.title.as_deref(),
            ),
            doi: row.doi,
            title: row.title,
        })
    }

    pub async fn cancel<S>(
        import_id: Uuid,
        uploaded_by: Uuid,
        store: &S,
        db: &PgPool,
    ) -> Result<(), PaperImportStagingError>
    where
        S: MediaObjectStore + ?Sized,
    {
        // cancellingを先に記録しておくことでGarage削除後にDB操作が失敗しても
        // 再試行対象を見失わない。S3互換DELETEは同じkeyへ再実行できる前提。
        let row = sqlx::query_as::<_, CancellingImportRow>(
            r#"
            UPDATE paper_imports
            SET status = 'cancelling', updated_at = now()
            WHERE id = $1
              AND uploaded_by = $2
              AND status IN (
                  'metadata_required', 'staged',
                  'extracting_occurrences', 'reviewing', 'cancelling'
              )
            RETURNING bucket, object_key
            "#,
        )
        .bind(import_id)
        .bind(uploaded_by)
        .fetch_optional(db)
        .await?
        .ok_or(PaperImportStagingError::NotFound)?;

        store
            .delete_object(DeleteMediaObjectInput {
                bucket: row.bucket,
                object_key: row.object_key,
            })
            .await
            .map_err(|_| PaperImportStagingError::ObjectStoreFailed)?;

        sqlx::query(
            r#"
            DELETE FROM paper_imports
            WHERE id = $1 AND uploaded_by = $2 AND status = 'cancelling'
            "#,
        )
        .bind(import_id)
        .bind(uploaded_by)
        .execute(db)
        .await?;

        Ok(())
    }
}

async fn find_active_staged_import(
    db: &PgPool,
    uploaded_by: Uuid,
    sha256: &str,
) -> Result<Option<StagedPaperImportRow>, sqlx::Error> {
    sqlx::query_as::<_, StagedPaperImportRow>(
        r#"
        SELECT id, original_filename, content_type, size_bytes, sha256,
               doi, title, authors, publication_year, journal,
               volume, issue, pages, article_number, status
        FROM paper_imports
        WHERE uploaded_by = $1
          AND sha256 = $2
          AND status IN (
              'metadata_required', 'staged',
              'extracting_occurrences', 'reviewing'
          )
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(uploaded_by)
    .bind(sha256)
    .fetch_optional(db)
    .await
}

fn normalize_start_input(
    mut input: StartPaperImportInput,
) -> Result<StartPaperImportInput, PaperImportStagingError> {
    input.bucket = input.bucket.trim().to_string();
    input.content_type = input.content_type.trim().to_ascii_lowercase();
    input.payload_sha256 = input.payload_sha256.trim().to_ascii_lowercase();

    if input.bucket.is_empty()
        || input.content_type != "application/pdf"
        || input.size_bytes == 0
        || input.size_bytes > PAPER_PDF_FILE_SIZE_LIMIT_BYTES
        || input.size_bytes > i64::MAX as u64
        || !is_valid_sha256_hex(&input.payload_sha256)
    {
        return Err(PaperImportStagingError::InvalidInput);
    }

    Ok(input)
}

fn output_from_new_staging(
    import_id: Uuid,
    original_filename: Option<String>,
    content_type: String,
    size_bytes: i64,
    sha256: String,
    metadata: GrobidPaperMetadata,
) -> StartPaperImportOutput {
    let requires_bibliographic_input =
        requires_bibliographic_input(metadata.doi.as_deref(), metadata.title.as_deref());

    StartPaperImportOutput {
        status: if requires_bibliographic_input {
            StartPaperImportStatus::MetadataRequired
        } else {
            StartPaperImportStatus::Staged
        },
        import_id: Some(import_id),
        paper_id: None,
        original_filename,
        content_type,
        size_bytes,
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
        requires_bibliographic_input,
    }
}

fn output_from_staged_row(row: StagedPaperImportRow) -> StartPaperImportOutput {
    let requires_bibliographic_input =
        requires_bibliographic_input(row.doi.as_deref(), row.title.as_deref());

    StartPaperImportOutput {
        status: if requires_bibliographic_input || row.status == "metadata_required" {
            StartPaperImportStatus::MetadataRequired
        } else {
            StartPaperImportStatus::Staged
        },
        import_id: Some(row.id),
        paper_id: None,
        original_filename: row.original_filename,
        content_type: row.content_type,
        size_bytes: row.size_bytes,
        sha256: row.sha256,
        doi: row.doi,
        title: row.title,
        authors: row.authors,
        publication_year: row.publication_year,
        journal: row.journal,
        volume: row.volume,
        issue: row.issue,
        pages: row.pages,
        article_number: row.article_number,
        requires_bibliographic_input,
    }
}

fn output_from_registered_paper(metadata: PaperMetadata) -> StartPaperImportOutput {
    let requires_bibliographic_input =
        requires_bibliographic_input(metadata.doi.as_deref(), metadata.title.as_deref());

    StartPaperImportOutput {
        status: StartPaperImportStatus::AlreadyImported,
        import_id: None,
        paper_id: Some(metadata.id),
        original_filename: metadata.original_filename,
        content_type: metadata.content_type,
        size_bytes: metadata.size_bytes,
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
        requires_bibliographic_input,
    }
}

fn requires_bibliographic_input(doi: Option<&str>, title: Option<&str>) -> bool {
    let has_doi = doi.is_some_and(|value| !value.trim().is_empty());
    let has_title = title.is_some_and(|value| !value.trim().is_empty());
    !has_doi && !has_title
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
