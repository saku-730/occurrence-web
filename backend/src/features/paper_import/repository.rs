use sqlx::PgPool;
use uuid::Uuid;

pub struct InsertPaperMetadata<'a> {
    pub id: Uuid,
    pub bucket: &'a str,
    pub object_key: &'a str,
    pub content_type: &'a str,
    pub size_bytes: i64,
    pub original_filename: Option<&'a str>,
    pub sha256: &'a str,
    pub doi: Option<&'a str>,
    pub title: Option<&'a str>,
    pub authors: Option<&'a str>,
    pub publication_year: Option<i32>,
    pub journal: Option<&'a str>,
    pub volume: Option<&'a str>,
    pub issue: Option<&'a str>,
    pub pages: Option<&'a str>,
    pub article_number: Option<&'a str>,
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
                   uploaded_by
            FROM papers
            WHERE sha256 = $1
            "#,
        )
        .bind(sha256)
        .fetch_optional(db)
        .await
    }

    pub async fn insert_if_sha256_absent(
        db: &PgPool,
        metadata: InsertPaperMetadata<'_>,
    ) -> Result<bool, sqlx::Error> {
        // handler側の事前重複確認だけでは同時upload時に競合できるため、
        // DBのUNIQUE(sha256)を最終防衛線としてON CONFLICTで吸収する。
        let result = sqlx::query(
            r#"
            INSERT INTO papers (
                id,
                bucket,
                object_key,
                content_type,
                size_bytes,
                original_filename,
                sha256,
                doi,
                title,
                authors,
                publication_year,
                journal,
                volume,
                issue,
                pages,
                article_number,
                uploaded_by
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $12, $13, $14, $15, $16,
                $17
            )
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
        .bind(metadata.doi)
        .bind(metadata.title)
        .bind(metadata.authors)
        .bind(metadata.publication_year)
        .bind(metadata.journal)
        .bind(metadata.volume)
        .bind(metadata.issue)
        .bind(metadata.pages)
        .bind(metadata.article_number)
        .bind(metadata.uploaded_by)
        .execute(db)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    pub async fn complete_missing_bibliographic_metadata(
        db: &PgPool,
        paper_id: Uuid,
        uploaded_by: Uuid,
        doi: Option<&str>,
        title: Option<&str>,
    ) -> Result<Option<PaperMetadata>, sqlx::Error> {
        // Preserve metadata extracted by GROBID. CASE is evaluated by
        // PostgreSQL during the UPDATE, so concurrent completion requests can
        // never replace a value that another request has already supplied.
        // Ownership belongs in the same predicate to prevent a check/update
        // race and to hide other users' paper identifiers.
        sqlx::query_as::<_, PaperMetadata>(
            r#"
            UPDATE papers
            SET doi = CASE
                    WHEN doi IS NULL OR BTRIM(doi) = '' THEN $3
                    ELSE doi
                END,
                title = CASE
                    WHEN title IS NULL OR BTRIM(title) = '' THEN $4
                    ELSE title
                END
            WHERE id = $1
              AND uploaded_by = $2
            RETURNING id, bucket, object_key, content_type, size_bytes,
                      original_filename, sha256,
                      doi, title, authors, publication_year, journal,
                      volume, issue, pages, article_number,
                      uploaded_by
            "#,
        )
        .bind(paper_id)
        .bind(uploaded_by)
        .bind(doi)
        .bind(title)
        .fetch_optional(db)
        .await
    }
}
