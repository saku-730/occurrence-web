use sqlx::PgPool;
use uuid::Uuid;

use super::repository::{InsertMediaMetadata, MediaRepository};

#[derive(Debug)]
pub enum MediaServiceError {
    InvalidInput,
    PayloadTooLarge,
    ObjectStoreFailed,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for MediaServiceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug, Clone)]
pub struct UploadMediaInput {
    pub app_base_url: String,
    pub bucket: String,
    pub uploaded_by: Uuid,
    pub original_filename: Option<String>,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadMediaOutput {
    pub media_id: Uuid,
    pub media_uri: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub original_filename: Option<String>,
    pub uploaded_by: Uuid,
}

#[derive(Debug, Clone)]
pub struct PutMediaObjectInput {
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DeleteMediaObjectInput {
    pub bucket: String,
    pub object_key: String,
}

#[async_trait::async_trait]
pub trait MediaObjectStore: Send + Sync {
    async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError>;
    async fn delete_object(&self, input: DeleteMediaObjectInput) -> Result<(), MediaServiceError>;
}

// 添付ファイル本体の上限。handlerのContent-Length検査は早期拒否用であり、
// 信頼できる最終判定はserviceが実際に受け取ったbyte数に対して行う。
const MEDIA_FILE_SIZE_LIMIT_BYTES: usize = 1000 * 1024 * 1024;

fn validate_media_size_bytes(size_bytes: usize) -> Result<(), MediaServiceError> {
    if size_bytes > MEDIA_FILE_SIZE_LIMIT_BYTES {
        return Err(MediaServiceError::PayloadTooLarge);
    }

    Ok(())
}

fn is_allowed_content_type(content_type: &str) -> bool {
    matches!(
        content_type.trim().to_ascii_lowercase().as_str(),
        // jpg/jpeg, png, webp
        "image/jpeg" | "image/png" | "image/webp"
            // mp3, wav, m4a
            | "audio/mpeg"
            | "audio/wav"
            | "audio/x-wav"
            | "audio/mp4"
            // mp4, mov
            | "video/mp4"
            | "video/quicktime"
    )
}

pub struct MediaService;

impl MediaService {
    pub async fn upload_media<S>(
        input: UploadMediaInput,
        store: &S,
        db: &PgPool,
    ) -> Result<UploadMediaOutput, MediaServiceError>
    where
        S: MediaObjectStore + ?Sized,
    {
        let app_base_url = input.app_base_url.trim().trim_end_matches('/');
        let bucket = input.bucket.trim();
        let content_type = input.content_type.trim();

        // object storageへ空objectや保存先不明のobjectを書かないよう、service境界で最低限の入力を弾く。
        // 拡張子/MIME/サイズ上限の詳細validationは、この正常系の次に個別テストで固める。
        if app_base_url.is_empty()
            || bucket.is_empty()
            || content_type.is_empty()
            || input.bytes.is_empty()
        {
            return Err(MediaServiceError::InvalidInput);
        }

        // spec/07_media.mdでMVP対象にした画像・音声・動画だけを受け付ける。
        // 許可外のMIME typeはGarageへ書き込む前に拒否し、不要なobjectや後始末を発生させない。
        if !is_allowed_content_type(content_type) {
            return Err(MediaServiceError::InvalidInput);
        }

        // Content-Lengthは省略や偽装が可能なので、保存直前に実データ長を必ず検証する。
        // この判定をobject storage呼び出しより前に置き、上限超過objectを作らない。
        validate_media_size_bytes(input.bytes.len())?;

        let media_id = Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let media_uri = format!("{app_base_url}/media/{media_id}");
        let size_bytes = input.bytes.len() as i64;

        // PostgreSQLのmedia_objectsへ保存するmetadataと同じ識別子を使ってobject keyを作る。
        // これにより、RDFで参照するmedia URI、PostgreSQLのid、Garage上のobjectを追跡しやすくする。
        store
            .put_object(PutMediaObjectInput {
                bucket: bucket.to_string(),
                object_key: object_key.clone(),
                content_type: content_type.to_string(),
                bytes: input.bytes,
            })
            .await?;

        // Garageへの保存が成功した後、公開URIの解決と所有者認可に必要なmetadataを永続化する。
        // PostgreSQL失敗時のGarage object補償削除は、delete API追加時に同じstore抽象へ実装する。
        let metadata_result = MediaRepository::insert(
            db,
            InsertMediaMetadata {
                id: media_id,
                bucket,
                object_key: &object_key,
                content_type,
                size_bytes,
                original_filename: input.original_filename.as_deref(),
                uploaded_by: input.uploaded_by,
            },
        )
        .await;

        if let Err(database_error) = metadata_result {
            // DBにmedia URIの解決情報が残らない場合、先に作成したGarage objectは孤立する。
            // 同じbucket/keyを補償削除し、削除成功後は原因だったDBエラーを呼び出し元へ返す。
            store
                .delete_object(DeleteMediaObjectInput {
                    bucket: bucket.to_string(),
                    object_key: object_key.clone(),
                })
                .await?;

            return Err(MediaServiceError::Database(database_error));
        }

        Ok(UploadMediaOutput {
            media_id,
            media_uri,
            bucket: bucket.to_string(),
            object_key,
            content_type: content_type.to_string(),
            size_bytes,
            original_filename: input.original_filename,
            uploaded_by: input.uploaded_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    #[derive(Debug, Default)]
    struct RecordingMediaObjectStore {
        written_objects: std::sync::Mutex<Vec<RecordedObjectWrite>>,
        deleted_objects: std::sync::Mutex<Vec<RecordedObjectDelete>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedObjectWrite {
        bucket: String,
        object_key: String,
        content_type: String,
        bytes: Vec<u8>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedObjectDelete {
        bucket: String,
        object_key: String,
    }

    #[async_trait::async_trait]
    impl MediaObjectStore for RecordingMediaObjectStore {
        async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
            self.written_objects
                .lock()
                .expect("recorded object writes lock should not be poisoned")
                .push(RecordedObjectWrite {
                    bucket: input.bucket,
                    object_key: input.object_key,
                    content_type: input.content_type,
                    bytes: input.bytes,
                });

            Ok(())
        }

        async fn delete_object(
            &self,
            input: DeleteMediaObjectInput,
        ) -> Result<(), MediaServiceError> {
            self.deleted_objects
                .lock()
                .expect("recorded media object deletes lock should not be poisoned")
                .push(RecordedObjectDelete {
                    bucket: input.bucket,
                    object_key: input.object_key,
                });

            Ok(())
        }
    }

    #[tokio::test]
    async fn upload_media_rejects_unsupported_content_type_and_does_not_write_object() {
        let store = RecordingMediaObjectStore::default();
        // 入力検証でDB到達前に失敗することも確認できるよう、接続を確立しないpoolを使う。
        let db = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1/unused")
            .expect("lazy PostgreSQL pool should be constructible");
        let result = MediaService::upload_media(
            UploadMediaInput {
                app_base_url: "https://bio-database.net".to_string(),
                bucket: "occurrence-media".to_string(),
                uploaded_by: Uuid::new_v4(),
                original_filename: Some("note.txt".to_string()),
                content_type: "text/plain".to_string(),
                bytes: b"plain text is not a supported media attachment".to_vec(),
            },
            &store,
            &db,
        )
        .await;

        assert!(
            matches!(result, Err(MediaServiceError::InvalidInput)),
            "unsupported content type should be rejected: {:?}",
            result
        );

        let writes = store
            .written_objects
            .lock()
            .expect("recorded object writes lock should not be poisoned");

        assert!(
            writes.is_empty(),
            "unsupported content type must not be written to object storage"
        );
    }

    #[test]
    fn media_size_validation_accepts_1000_mb_and_rejects_1001_mb() {
        const MEBIBYTE: usize = 1024 * 1024;

        assert!(
            validate_media_size_bytes(1000 * MEBIBYTE).is_ok(),
            "an attachment exactly at the 1000MB limit should be accepted"
        );
        assert!(matches!(
            validate_media_size_bytes(1001 * MEBIBYTE),
            Err(MediaServiceError::PayloadTooLarge)
        ));
    }

    #[tokio::test]
    async fn upload_media_writes_attachment_object_and_returns_media_metadata() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for media tests");
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("PostgreSQL should be available for media tests");
        let email = format!("media-output-{}@example.com", Uuid::new_v4());
        let uploaded_by = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (email, user_name, password_hash) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(email)
        .bind("media-output-user")
        .bind("not-used-by-this-test")
        .fetch_one(&db)
        .await
        .expect("media output test user should be inserted");

        let store = RecordingMediaObjectStore::default();
        let bytes = b"fake-jpeg-bytes".to_vec();

        let output = MediaService::upload_media(
            UploadMediaInput {
                app_base_url: "https://bio-database.net".to_string(),
                bucket: "occurrence-media".to_string(),
                uploaded_by,
                original_filename: Some("sample.jpg".to_string()),
                content_type: "image/jpeg".to_string(),
                bytes: bytes.clone(),
            },
            &store,
            &db,
        )
        .await
        .expect("valid attachment upload should succeed");

        assert_eq!(output.bucket, "occurrence-media");
        assert_eq!(output.content_type, "image/jpeg");
        assert_eq!(output.size_bytes, bytes.len() as i64);
        assert_eq!(output.original_filename.as_deref(), Some("sample.jpg"));
        assert_eq!(output.uploaded_by, uploaded_by);
        assert_eq!(
            output.media_uri,
            format!("https://bio-database.net/media/{}", output.media_id)
        );
        assert_eq!(output.object_key, format!("media/{}", output.media_id));

        let writes = store
            .written_objects
            .lock()
            .expect("recorded object writes lock should not be poisoned");

        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].bucket, "occurrence-media");
        assert_eq!(writes[0].object_key, output.object_key);
        assert_eq!(writes[0].content_type, "image/jpeg");
        assert_eq!(writes[0].bytes, bytes);
        drop(writes);

        // metadataの外部キーを考慮し、子レコードから削除してテスト間の状態を分離する。
        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(output.media_id)
            .execute(&db)
            .await
            .expect("media output metadata should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(uploaded_by)
            .execute(&db)
            .await
            .expect("media output test user should be removed after test");
    }

    #[tokio::test]
    async fn upload_media_deletes_garage_object_when_postgresql_metadata_save_fails() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for media tests");
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("PostgreSQL should be available for media tests");

        let store = RecordingMediaObjectStore::default();
        // usersに存在しないUUIDを指定し、Garage PUT後のmetadata INSERTだけを確実に失敗させる。
        let result = MediaService::upload_media(
            UploadMediaInput {
                app_base_url: "https://bio-database.net".to_string(),
                bucket: "occurrence-media".to_string(),
                uploaded_by: Uuid::new_v4(),
                original_filename: Some("rollback.jpg".to_string()),
                content_type: "image/jpeg".to_string(),
                bytes: b"garage-compensation-test".to_vec(),
            },
            &store,
            &db,
        )
        .await;

        assert!(matches!(result, Err(MediaServiceError::Database(_))));

        let writes = store
            .written_objects
            .lock()
            .expect("recorded media object writes lock should not be poisoned");
        assert_eq!(
            writes.len(),
            1,
            "Garage PUT should succeed before DB failure"
        );
        let written_object = writes[0].clone();
        drop(writes);

        let deletes = store
            .deleted_objects
            .lock()
            .expect("recorded media object deletes lock should not be poisoned");
        assert_eq!(deletes.len(), 1, "failed metadata save must be compensated");
        assert_eq!(deletes[0].bucket, written_object.bucket);
        assert_eq!(deletes[0].object_key, written_object.object_key);
    }

    #[tokio::test]
    async fn upload_media_saves_metadata_to_postgresql() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for media tests");
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("PostgreSQL should be available for media tests");

        let email = format!("media-service-{}@example.com", Uuid::new_v4());
        let uploaded_by = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
        )
        .bind(&email)
        .bind("media-service-user")
        .bind("not-used-by-this-test")
        .fetch_one(&db)
        .await
        .expect("media test user should be inserted");

        let store = RecordingMediaObjectStore::default();
        let bytes = b"postgresql-metadata-test-image".to_vec();
        let output = MediaService::upload_media(
            UploadMediaInput {
                app_base_url: "https://bio-database.net".to_string(),
                bucket: "occurrence-media".to_string(),
                uploaded_by,
                original_filename: Some("metadata.jpg".to_string()),
                content_type: "image/jpeg".to_string(),
                bytes: bytes.clone(),
            },
            &store,
            &db,
        )
        .await
        .expect("valid upload should save metadata");

        // serviceの戻り値だけではなく、PostgreSQLを直接参照して永続化を確認する。
        let metadata =
            sqlx::query_as::<_, (Uuid, String, String, String, i64, Option<String>, Uuid)>(
                r#"
            SELECT id, bucket, object_key, content_type, size_bytes, original_filename, uploaded_by
            FROM media_objects
            WHERE id = $1
            "#,
            )
            .bind(output.media_id)
            .fetch_one(&db)
            .await
            .expect("saved media metadata should be queryable");

        assert_eq!(metadata.0, output.media_id);
        assert_eq!(metadata.1, "occurrence-media");
        assert_eq!(metadata.2, output.object_key);
        assert_eq!(metadata.3, "image/jpeg");
        assert_eq!(metadata.4, bytes.len() as i64);
        assert_eq!(metadata.5.as_deref(), Some("metadata.jpg"));
        assert_eq!(metadata.6, uploaded_by);

        // 他テストへ永続データを残さないよう、外部キーの子から順に後始末する。
        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(output.media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(uploaded_by)
            .execute(&db)
            .await
            .expect("media test user should be removed after test");
    }
}
