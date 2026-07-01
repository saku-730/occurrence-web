use std::{
    path::{Path, PathBuf},
    pin::Pin,
};

use axum::body::Bytes;
use futures_util::Stream;

use sqlx::PgPool;
use uuid::Uuid;

use super::repository::{InsertMediaMetadata, MediaMetadata, MediaRepository};

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
    // handlerがchunk単位で作成した一時ファイル。service完了まで呼び出し元が寿命を保持する。
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub payload_sha256: String,
    // inferはファイル先頭のsignatureを使うため、全ファイルではなくprobeだけを渡す。
    pub mime_probe: Vec<u8>,
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
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub payload_sha256: String,
}

#[derive(Debug, Clone)]
pub struct GetMediaObjectInput {
    pub bucket: String,
    pub object_key: String,
}

pub type MediaObjectByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, MediaServiceError>> + Send>>;

pub struct GetMediaOutput {
    pub media_id: Uuid,
    pub content_type: String,
    pub size_bytes: i64,
    pub original_filename: Option<String>,
    pub uploaded_by: Uuid,
    pub stream: MediaObjectByteStream,
}

#[derive(Debug, Clone)]
pub struct DeleteMediaObjectInput {
    pub bucket: String,
    pub object_key: String,
}

#[async_trait::async_trait]
pub trait MediaObjectStore: Send + Sync {
    async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError>;
    async fn get_object(
        &self,
        input: GetMediaObjectInput,
    ) -> Result<MediaObjectByteStream, MediaServiceError>;
    async fn delete_object(&self, input: DeleteMediaObjectInput) -> Result<(), MediaServiceError>;
}

// 添付ファイル本体の上限。handlerのContent-Length検査は早期拒否用であり、
// 信頼できる最終判定はserviceが実際に受け取ったbyte数に対して行う。
pub const MEDIA_FILE_SIZE_LIMIT_BYTES: u64 = 1000 * 1024 * 1024;

fn validate_media_size_bytes(size_bytes: u64) -> Result<(), MediaServiceError> {
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

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn output_from_existing_metadata(app_base_url: &str, metadata: MediaMetadata) -> UploadMediaOutput {
    UploadMediaOutput {
        media_id: metadata.id,
        media_uri: format!("{app_base_url}/media/{}", metadata.id),
        bucket: metadata.bucket,
        object_key: metadata.object_key,
        content_type: metadata.content_type,
        size_bytes: metadata.size_bytes,
        original_filename: metadata.original_filename,
        uploaded_by: metadata.uploaded_by,
    }
}

fn filename_extension_matches_content_type(
    original_filename: Option<&str>,
    content_type: &str,
) -> bool {
    let Some(filename) = original_filename
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return false;
    };
    let Some(extension) = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
    else {
        return false;
    };
    let extension = extension.to_ascii_lowercase();

    matches!(
        (
            content_type.trim().to_ascii_lowercase().as_str(),
            extension.as_str()
        ),
        ("image/jpeg", "jpg" | "jpeg")
            | ("image/png", "png")
            | ("image/webp", "webp")
            | ("audio/mpeg", "mp3")
            | ("audio/wav" | "audio/x-wav", "wav")
            | ("audio/mp4", "m4a")
            | ("video/mp4", "mp4")
            | ("video/quicktime", "mov")
    )
}

fn detected_content_type_matches(declared_content_type: &str, bytes: &[u8]) -> bool {
    let Some(detected) = infer::get(bytes) else {
        // 判定不能なデータを申告値だけで許可すると偽装を防げないため拒否する。
        return false;
    };

    let declared = declared_content_type.trim().to_ascii_lowercase();
    let detected = detected.mime_type();

    declared == detected
        || matches!(
            (declared.as_str(), detected),
            // inferのWAV/M4A表記とHTTPで一般的に使われる許可MIMEの差だけを吸収する。
            ("audio/wav", "audio/x-wav") | ("audio/mp4", "audio/m4a")
        )
}

pub struct MediaService;

impl MediaService {
    pub async fn get_media<S>(
        media_id: Uuid,
        store: &S,
        db: &PgPool,
    ) -> Result<Option<GetMediaOutput>, MediaServiceError>
    where
        S: MediaObjectStore + ?Sized,
    {
        let Some(metadata) = MediaRepository::find_by_id(db, media_id).await? else {
            return Ok(None);
        };

        // PostgreSQLをobject storageの索引として使い、bucket/keyをクライアント入力から受け取らない。
        let stream = store
            .get_object(GetMediaObjectInput {
                bucket: metadata.bucket,
                object_key: metadata.object_key,
            })
            .await?;

        Ok(Some(GetMediaOutput {
            media_id: metadata.id,
            content_type: metadata.content_type,
            size_bytes: metadata.size_bytes,
            original_filename: metadata.original_filename,
            uploaded_by: metadata.uploaded_by,
            stream,
        }))
    }

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
        let payload_sha256 = input.payload_sha256.trim().to_ascii_lowercase();

        // object storageへ空objectや保存先不明のobjectを書かないよう、service境界で最低限の入力を弾く。
        // 拡張子/MIME/サイズ上限の詳細validationは、この正常系の次に個別テストで固める。
        if app_base_url.is_empty()
            || bucket.is_empty()
            || content_type.is_empty()
            || input.size_bytes == 0
            || input.mime_probe.is_empty()
            || !is_valid_sha256_hex(&payload_sha256)
        {
            return Err(MediaServiceError::InvalidInput);
        }

        // spec/07_media.mdでMVP対象にした画像・音声・動画だけを受け付ける。
        // 許可外のMIME typeはGarageへ書き込む前に拒否し、不要なobjectや後始末を発生させない。
        if !is_allowed_content_type(content_type) {
            return Err(MediaServiceError::InvalidInput);
        }

        // multipartのContent-Typeは送信者が自由に指定できるため、magic bytesから検出した形式とも照合する。
        // 許可形式でも実データと一致しない場合はGarageへ送る前に拒否する。
        if !detected_content_type_matches(content_type, &input.mime_probe) {
            return Err(MediaServiceError::InvalidInput);
        }

        // 元ファイル名はmetadata用途だが、拡張子偽装を見逃さないためMIMEとの組み合わせも検証する。
        // 大文字拡張子は許可する一方、二重拡張子は最後の拡張子だけで判断する。
        if !filename_extension_matches_content_type(
            input.original_filename.as_deref(),
            content_type,
        ) {
            return Err(MediaServiceError::InvalidInput);
        }

        // Content-Lengthは省略や偽装が可能なので、保存直前に実データ長を必ず検証する。
        // この判定をobject storage呼び出しより前に置き、上限超過objectを作らない。
        validate_media_size_bytes(input.size_bytes)?;

        // 同じユーザーが同一bytesを再送した場合は、論理mediaとGarage objectを再利用する。
        // ユーザーを検索条件へ含め、他人のmedia URIや所有権を共有しない。
        if let Some(existing) =
            MediaRepository::find_by_uploader_and_sha256(db, input.uploaded_by, &payload_sha256)
                .await?
        {
            return Ok(output_from_existing_metadata(app_base_url, existing));
        }

        let media_id = Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let media_uri = format!("{app_base_url}/media/{media_id}");
        let size_bytes =
            i64::try_from(input.size_bytes).map_err(|_| MediaServiceError::PayloadTooLarge)?;

        // PostgreSQLのmedia_objectsへ保存するmetadataと同じ識別子を使ってobject keyを作る。
        // これにより、RDFで参照するmedia URI、PostgreSQLのid、Garage上のobjectを追跡しやすくする。
        store
            .put_object(PutMediaObjectInput {
                bucket: bucket.to_string(),
                object_key: object_key.clone(),
                content_type: content_type.to_string(),
                file_path: input.file_path,
                size_bytes: input.size_bytes,
                payload_sha256: payload_sha256.clone(),
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
                sha256: &payload_sha256,
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

            // 同じファイルが並行uploadされ、一意index競合になった場合は勝った既存行を返す。
            // 競合側のGarage objectは上で削除済みなので、物理objectも1つだけ残る。
            let is_unique_violation = database_error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation());
            if is_unique_violation {
                if let Some(existing) = MediaRepository::find_by_uploader_and_sha256(
                    db,
                    input.uploaded_by,
                    &payload_sha256,
                )
                .await?
                {
                    return Ok(output_from_existing_metadata(app_base_url, existing));
                }
            }

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
    use std::io::Write;
    use std::sync::Arc;

    use super::*;
    use futures_util::StreamExt;
    use sha2::{Digest, Sha256};
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    // inferがJPEGとして判定できる最小限のsignatureを正常系fixtureで共用する。
    const TEST_JPEG_BYTES: &[u8] = &[0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0x00];

    fn test_upload_input(
        bytes: &[u8],
        uploaded_by: Uuid,
        original_filename: &str,
        content_type: &str,
    ) -> (tempfile::TempPath, UploadMediaInput) {
        let mut file = tempfile::NamedTempFile::new().expect("test temporary file should open");
        file.write_all(bytes)
            .expect("test temporary file should be writable");
        let temp_path = file.into_temp_path();
        let input = UploadMediaInput {
            app_base_url: "https://bio-database.net".to_string(),
            bucket: "occurrence-media".to_string(),
            uploaded_by,
            original_filename: Some(original_filename.to_string()),
            content_type: content_type.to_string(),
            file_path: temp_path.to_path_buf(),
            size_bytes: bytes.len() as u64,
            payload_sha256: hex::encode(Sha256::digest(bytes)),
            mime_probe: bytes[..bytes.len().min(8192)].to_vec(),
        };

        (temp_path, input)
    }

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
            let bytes = tokio::fs::read(&input.file_path)
                .await
                .map_err(|_| MediaServiceError::ObjectStoreFailed)?;
            self.written_objects
                .lock()
                .expect("recorded object writes lock should not be poisoned")
                .push(RecordedObjectWrite {
                    bucket: input.bucket,
                    object_key: input.object_key,
                    content_type: input.content_type,
                    bytes,
                });

            Ok(())
        }

        async fn get_object(
            &self,
            _input: GetMediaObjectInput,
        ) -> Result<MediaObjectByteStream, MediaServiceError> {
            Err(MediaServiceError::ObjectStoreFailed)
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

    #[derive(Clone)]
    struct ReadableMediaObjectStore {
        expected_bucket: String,
        expected_object_key: String,
        bytes: Arc<Vec<u8>>,
    }

    #[async_trait::async_trait]
    impl MediaObjectStore for ReadableMediaObjectStore {
        async fn put_object(&self, _input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
            Ok(())
        }

        async fn get_object(
            &self,
            input: GetMediaObjectInput,
        ) -> Result<MediaObjectByteStream, MediaServiceError> {
            assert_eq!(input.bucket, self.expected_bucket);
            assert_eq!(input.object_key, self.expected_object_key);
            let bytes = self.bytes.clone();
            Ok(Box::pin(futures_util::stream::once(async move {
                Ok(Bytes::copy_from_slice(bytes.as_slice()))
            })))
        }

        async fn delete_object(
            &self,
            _input: DeleteMediaObjectInput,
        ) -> Result<(), MediaServiceError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn get_media_returns_metadata_and_object_stream_for_existing_media() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for media tests");
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("PostgreSQL should be available for media tests");
        let email = format!("media-get-{}@example.com", Uuid::new_v4());
        let uploaded_by = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (email, user_name, password_hash) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(email)
        .bind("media-get-user")
        .bind("not-used-by-this-test")
        .fetch_one(&db)
        .await
        .expect("media get test user should be inserted");
        let media_id = Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("retrieved.jpg")
        .bind(uploaded_by)
        .bind(hex::encode(Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let store = ReadableMediaObjectStore {
            expected_bucket: "occurrence-media".to_string(),
            expected_object_key: object_key,
            bytes: Arc::new(TEST_JPEG_BYTES.to_vec()),
        };

        let output = MediaService::get_media(media_id, &store, &db)
            .await
            .expect("media get should succeed")
            .expect("existing media should be returned");

        assert_eq!(output.media_id, media_id);
        assert_eq!(output.content_type, "image/jpeg");
        assert_eq!(output.size_bytes, TEST_JPEG_BYTES.len() as i64);
        assert_eq!(output.original_filename.as_deref(), Some("retrieved.jpg"));
        let mut stream = output.stream;
        let mut actual_bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            actual_bytes.extend_from_slice(&chunk.expect("object stream chunk should succeed"));
        }
        assert_eq!(actual_bytes, TEST_JPEG_BYTES);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media get test metadata should be removed");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(uploaded_by)
            .execute(&db)
            .await
            .expect("media get test user should be removed");
    }

    #[tokio::test]
    async fn upload_media_rejects_unsupported_content_type_and_does_not_write_object() {
        let store = RecordingMediaObjectStore::default();
        // 入力検証でDB到達前に失敗することも確認できるよう、接続を確立しないpoolを使う。
        let db = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1/unused")
            .expect("lazy PostgreSQL pool should be constructible");
        let (_temp_path, input) = test_upload_input(
            b"plain text is not a supported media attachment",
            Uuid::new_v4(),
            "note.txt",
            "text/plain",
        );
        let result = MediaService::upload_media(input, &store, &db).await;

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

    #[tokio::test]
    async fn upload_media_rejects_content_when_detected_mime_does_not_match_declared_mime() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for media tests");
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("PostgreSQL should be available for media tests");
        let store = RecordingMediaObjectStore::default();

        let (_temp_path, input) = test_upload_input(
            b"%PDF-1.7 disguised as a JPEG",
            Uuid::new_v4(),
            "disguised.jpg",
            "image/jpeg",
        );
        let result = MediaService::upload_media(input, &store, &db).await;

        assert!(
            matches!(result, Err(MediaServiceError::InvalidInput)),
            "declared MIME must match the format detected from file bytes: {:?}",
            result
        );

        let writes = store
            .written_objects
            .lock()
            .expect("recorded media object writes lock should not be poisoned");
        assert!(
            writes.is_empty(),
            "disguised content must be rejected before Garage PUT"
        );
    }

    #[tokio::test]
    async fn upload_media_rejects_filename_extension_that_does_not_match_mime() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for media tests");
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("PostgreSQL should be available for media tests");
        let store = RecordingMediaObjectStore::default();
        let (_temp_path, input) = test_upload_input(
            TEST_JPEG_BYTES,
            Uuid::new_v4(),
            "jpeg-content-with-png-extension.png",
            "image/jpeg",
        );

        let result = MediaService::upload_media(input, &store, &db).await;

        assert!(
            matches!(result, Err(MediaServiceError::InvalidInput)),
            "filename extension must match the declared and detected MIME: {:?}",
            result
        );
        let writes = store
            .written_objects
            .lock()
            .expect("recorded object writes lock should not be poisoned");
        assert!(
            writes.is_empty(),
            "extension mismatch must be rejected before Garage PUT"
        );
    }

    #[test]
    fn media_size_validation_accepts_1000_mb_and_rejects_1001_mb() {
        const MEBIBYTE: u64 = 1024 * 1024;

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
        let bytes = TEST_JPEG_BYTES.to_vec();

        let (_temp_path, input) =
            test_upload_input(&bytes, uploaded_by, "sample.jpg", "image/jpeg");
        let output = MediaService::upload_media(input, &store, &db)
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
    async fn upload_media_reuses_existing_media_for_same_user_and_sha256() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for media tests");
        let db = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("PostgreSQL should be available for media tests");
        let email = format!("media-dedup-{}@example.com", Uuid::new_v4());
        let uploaded_by = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (email, user_name, password_hash) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(email)
        .bind("media-dedup-user")
        .bind("not-used-by-this-test")
        .fetch_one(&db)
        .await
        .expect("media dedup test user should be inserted");
        let store = RecordingMediaObjectStore::default();

        let (_first_temp_path, first_input) =
            test_upload_input(TEST_JPEG_BYTES, uploaded_by, "first.jpg", "image/jpeg");
        let first = MediaService::upload_media(first_input, &store, &db)
            .await
            .expect("first upload should succeed");

        let (_second_temp_path, second_input) =
            test_upload_input(TEST_JPEG_BYTES, uploaded_by, "renamed.jpg", "image/jpeg");
        let second = MediaService::upload_media(second_input, &store, &db)
            .await
            .expect("duplicate upload should return existing media");

        assert_eq!(second.media_id, first.media_id);
        assert_eq!(second.object_key, first.object_key);
        let writes = store
            .written_objects
            .lock()
            .expect("recorded object writes lock should not be poisoned");
        assert_eq!(writes.len(), 1, "duplicate bytes must not be PUT twice");
        drop(writes);

        let row_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM media_objects WHERE uploaded_by = $1",
        )
        .bind(uploaded_by)
        .fetch_one(&db)
        .await
        .expect("media row count should be queryable");
        assert_eq!(row_count, 1);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(first.media_id)
            .execute(&db)
            .await
            .expect("deduplicated media metadata should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(uploaded_by)
            .execute(&db)
            .await
            .expect("media dedup test user should be removed after test");
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
        let (_temp_path, input) = test_upload_input(
            TEST_JPEG_BYTES,
            Uuid::new_v4(),
            "rollback.jpg",
            "image/jpeg",
        );
        let result = MediaService::upload_media(input, &store, &db).await;

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
        let bytes = TEST_JPEG_BYTES.to_vec();
        let (_temp_path, input) =
            test_upload_input(&bytes, uploaded_by, "metadata.jpg", "image/jpeg");
        let output = MediaService::upload_media(input, &store, &db)
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
