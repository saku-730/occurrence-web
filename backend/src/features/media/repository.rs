use sqlx::PgPool;
use uuid::Uuid;

// PostgreSQLへ保存するmetadataだけを受け取る。ファイル本体はGarageの責務なので含めない。
pub struct InsertMediaMetadata<'a> {
    pub id: Uuid,
    pub bucket: &'a str,
    pub object_key: &'a str,
    pub content_type: &'a str,
    pub size_bytes: i64,
    pub original_filename: Option<&'a str>,
    pub uploaded_by: Uuid,
    pub sha256: &'a str,
}

#[derive(Debug, sqlx::FromRow)]
pub struct MediaMetadata {
    pub id: Uuid,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub original_filename: Option<String>,
    pub uploaded_by: Uuid,
}

pub struct MediaRepository;

impl MediaRepository {
    pub async fn find_by_uploader_and_sha256(
        db: &PgPool,
        uploaded_by: Uuid,
        sha256: &str,
    ) -> Result<Option<MediaMetadata>, sqlx::Error> {
        sqlx::query_as::<_, MediaMetadata>(
            r#"
            SELECT id, bucket, object_key, content_type, size_bytes,
                   original_filename, uploaded_by
            FROM media_objects
            WHERE uploaded_by = $1
              AND sha256 = $2
            "#,
        )
        .bind(uploaded_by)
        .bind(sha256)
        .fetch_optional(db)
        .await
    }

    pub async fn insert(db: &PgPool, metadata: InsertMediaMetadata<'_>) -> Result<(), sqlx::Error> {
        // media URI、Garage object、DB rowを同じUUIDで追跡できるよう、service生成IDをそのまま保存する。
        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id,
                bucket,
                object_key,
                content_type,
                size_bytes,
                original_filename,
                uploaded_by,
                sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(metadata.id)
        .bind(metadata.bucket)
        .bind(metadata.object_key)
        .bind(metadata.content_type)
        .bind(metadata.size_bytes)
        .bind(metadata.original_filename)
        .bind(metadata.uploaded_by)
        .bind(metadata.sha256)
        .execute(db)
        .await?;

        Ok(())
    }
}
