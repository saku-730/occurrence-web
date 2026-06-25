use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadMediaResponse {
    pub media_id: Uuid,
    pub media_uri: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub original_filename: Option<String>,
}
