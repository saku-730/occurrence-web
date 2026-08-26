use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, ToSchema)]
pub struct ReceivePaperPdfRequest {
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReceivePaperPdfResponse {
    pub status: String,
    pub paper_id: Uuid,
    pub original_filename: Option<String>,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub doi: Option<String>,
    pub title: Option<String>,
    pub requires_bibliographic_input: bool,
    pub authors: Option<String>,
    pub publication_year: Option<i32>,
    pub journal: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub article_number: Option<String>,
    pub message: String,
}
