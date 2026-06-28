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
}

pub struct MediaRepository;

impl MediaRepository {
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
                uploaded_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(metadata.id)
        .bind(metadata.bucket)
        .bind(metadata.object_key)
        .bind(metadata.content_type)
        .bind(metadata.size_bytes)
        .bind(metadata.original_filename)
        .bind(metadata.uploaded_by)
        .execute(db)
        .await?;

        Ok(())
    }
}
