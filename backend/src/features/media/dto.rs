use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, ToSchema)]
pub struct UploadMediaRequest {
    // OpenAPIではmultipart fileをstring/binaryとして表現する。実際の受信はAxum Multipartが担当する。
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

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
