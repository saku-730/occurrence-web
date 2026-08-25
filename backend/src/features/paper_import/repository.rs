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
    pub uploaded_by: Uuid,
}

#[derive(Debug, sqlx::FromRow)]
pub struct PaperMetadata {
    pub id: Uuid,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub original_filename: Option<String>,
    pub sha256: String,
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
                   original_filename, sha256, uploaded_by
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
                uploaded_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
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
        .execute(db)
        .await?;

        Ok(result.rows_affected() == 1)
    }
}
