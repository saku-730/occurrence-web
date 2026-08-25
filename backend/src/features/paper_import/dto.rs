use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, ToSchema)]
pub struct ReceivePaperPdfRequest {
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReceivePaperPdfResponse {
    pub original_filename: Option<String>,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub message: String,
}
