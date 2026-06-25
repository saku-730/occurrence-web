use uuid::Uuid;

#[derive(Debug)]
pub enum MediaServiceError {
    InvalidInput,
    ObjectStoreFailed,
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

#[async_trait::async_trait]
pub trait MediaObjectStore: Send + Sync {
    async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError>;
}

pub struct MediaService;

impl MediaService {
    pub async fn upload_media<S>(
        input: UploadMediaInput,
        store: &S,
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
    use uuid::Uuid;

    #[derive(Debug, Default)]
    struct RecordingMediaObjectStore {
        written_objects: std::sync::Mutex<Vec<RecordedObjectWrite>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedObjectWrite {
        bucket: String,
        object_key: String,
        content_type: String,
        bytes: Vec<u8>,
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
    }

    #[tokio::test]
    async fn upload_media_writes_attachment_object_and_returns_media_metadata() {
        let store = RecordingMediaObjectStore::default();
        let uploaded_by = Uuid::new_v4();
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
    }
}
