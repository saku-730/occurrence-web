use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct PrepareOccurrenceResponse {
    pub occurrence_id: String,
    pub occurrence_uri: String,
    pub nquads: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateOccurrenceResponse {
    pub occurrence_id: String,
    pub occurrence_uri: String,
}