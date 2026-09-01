use sqlx::PgPool;
use uuid::Uuid;

use super::grobid::GrobidPaperMetadata;

pub const PAPER_STATUS_UNREGISTERED: &str = "unregistered";
pub const PAPER_STATUS_REGISTERED: &str = "registered";

pub struct InsertPaperMetadata<'a> {
    pub id: Uuid,
    pub bucket: &'a str,
    pub object_key: &'a str,
    pub content_type: &'a str,
    pub size_bytes: i64,
    pub original_filename: Option<&'a str>,
    pub sha256: &'a str,
    pub uploaded_by: Uuid,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PaperMetadata {
    pub id: Uuid,
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
    pub uploaded_by: Uuid,
    pub status: String,
}

pub struct PaperRepository;

impl PaperRepository {
    pub async fn find_by_sha256(
        db: &PgPool,
        sha256: &str,
    ) -> Result<Option<PaperMetadata>, sqlx::Error> {
        sqlx::query_as::<_, PaperMetadata>(
            r#"
            SELECT id, bucket, object_key, content_type, size_bytes,
                   original_filename, sha256,
                   doi, title, authors, publication_year, journal,
                   volume, issue, pages, article_number,
                   uploaded_by, status
            FROM papers
            WHERE sha256 = $1
            "#,
        )
        .bind(sha256)
        .fetch_optional(db)
        .await
    }

    pub async fn find_by_id(
        db: &PgPool,
        paper_id: Uuid,
    ) -> Result<Option<PaperMetadata>, sqlx::Error> {
        sqlx::query_as::<_, PaperMetadata>(
            r#"
            SELECT id, bucket, object_key, content_type, size_bytes,
                   original_filename, sha256,
                   doi, title, authors, publication_year, journal,
                   volume, issue, pages, article_number,
                   uploaded_by, status
            FROM papers
            WHERE id = $1
            "#,
        )
        .bind(paper_id)
        .fetch_optional(db)
        .await
    }

    pub async fn insert_unregistered_if_sha256_absent(
        db: &PgPool,
        metadata: InsertPaperMetadata<'_>,
    ) -> Result<bool, sqlx::Error> {
        // The SHA-256 UNIQUE constraint is the final race guard. If another upload
        // inserts the same PDF first, the caller removes its just-created Garage object.
        let result = sqlx::query(
            r#"
            INSERT INTO papers (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, sha256, uploaded_by, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (sha256) DO NOTHING
            "#,
        )
        .bind(metadata.id)
        .bind(metadata.bucket)
        .bind(metadata.object_key)
        .bind(metadata.content_type)
        .bind(metadata.size_bytes)
        .bind(metadata.original_filename)
        .bind(metadata.sha256)
        .bind(metadata.uploaded_by)
        .bind(PAPER_STATUS_UNREGISTERED)
        .execute(db)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    pub async fn fill_missing_grobid_metadata(
        db: &PgPool,
        paper_id: Uuid,
        metadata: &GrobidPaperMetadata,
    ) -> Result<Option<PaperMetadata>, sqlx::Error> {
        sqlx::query_as::<_, PaperMetadata>(
            r#"
            UPDATE papers
            SET doi = CASE WHEN doi IS NULL OR BTRIM(doi) = '' THEN $2 ELSE doi END,
                title = CASE WHEN title IS NULL OR BTRIM(title) = '' THEN $3 ELSE title END,
                authors = CASE WHEN authors IS NULL OR BTRIM(authors) = '' THEN $4 ELSE authors END,
                publication_year = COALESCE(publication_year, $5),
                journal = CASE WHEN journal IS NULL OR BTRIM(journal) = '' THEN $6 ELSE journal END,
                volume = CASE WHEN volume IS NULL OR BTRIM(volume) = '' THEN $7 ELSE volume END,
                issue = CASE WHEN issue IS NULL OR BTRIM(issue) = '' THEN $8 ELSE issue END,
                pages = CASE WHEN pages IS NULL OR BTRIM(pages) = '' THEN $9 ELSE pages END,
                article_number = CASE
                    WHEN article_number IS NULL OR BTRIM(article_number) = '' THEN $10
                    ELSE article_number
                END
            WHERE id = $1
            RETURNING id, bucket, object_key, content_type, size_bytes,
                      original_filename, sha256,
                      doi, title, authors, publication_year, journal,
                      volume, issue, pages, article_number,
                      uploaded_by, status
            "#,
        )
        .bind(paper_id)
        .bind(metadata.doi.as_deref())
        .bind(metadata.title.as_deref())
        .bind(metadata.authors.as_deref())
        .bind(metadata.publication_year)
        .bind(metadata.journal.as_deref())
        .bind(metadata.volume.as_deref())
        .bind(metadata.issue.as_deref())
        .bind(metadata.pages.as_deref())
        .bind(metadata.article_number.as_deref())
        .fetch_optional(db)
        .await
    }

    pub async fn complete_missing_bibliographic_metadata(
        db: &PgPool,
        paper_id: Uuid,
        doi: Option<&str>,
        title: Option<&str>,
    ) -> Result<Option<PaperMetadata>, sqlx::Error> {
        sqlx::query_as::<_, PaperMetadata>(
            r#"
            UPDATE papers
            SET doi = CASE WHEN doi IS NULL OR BTRIM(doi) = '' THEN $2 ELSE doi END,
                title = CASE WHEN title IS NULL OR BTRIM(title) = '' THEN $3 ELSE title END
            WHERE id = $1
            RETURNING id, bucket, object_key, content_type, size_bytes,
                      original_filename, sha256,
                      doi, title, authors, publication_year, journal,
                      volume, issue, pages, article_number,
                      uploaded_by, status
            "#,
        )
        .bind(paper_id)
        .bind(doi)
        .bind(title)
        .fetch_optional(db)
        .await
    }

    pub async fn mark_registered(db: &PgPool, paper_id: Uuid) -> Result<bool, sqlx::Error> {
        // Reprocessing an already registered paper never changes it back to
        // unregistered. This method is only called after occurrence persistence succeeds.
        let result = sqlx::query(
            r#"
            UPDATE papers
            SET status = $2
            WHERE id = $1
            "#,
        )
        .bind(paper_id)
        .bind(PAPER_STATUS_REGISTERED)
        .execute(db)
        .await?;

        Ok(result.rows_affected() == 1)
    }
}
