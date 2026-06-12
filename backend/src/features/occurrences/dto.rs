use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

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

#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct SearchOccurrenceItem {
    pub occurrence_id: String,
    pub occurrence_uri: String,
    #[serde(skip_serializing)]
    pub creator_user_id: Option<Uuid>,
    pub scientific_name: Option<String>,
    pub basis_of_record: Option<String>,
    pub recorded_by: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub access_rights: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct SearchOccurrencesPage {
    pub limit: u32,
    pub next_cursor: Option<String>,
    pub has_next: bool,
}

#[derive(Debug, Serialize, PartialEq, ToSchema)]
pub struct SearchOccurrencesResponse {
    pub items: Vec<SearchOccurrenceItem>,
    pub page: SearchOccurrencesPage,
}


#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchOccurrencesRequest {
    pub filters: Vec<SearchOccurrenceFilter>,
    pub page: SearchOccurrencesRequestPage,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchOccurrenceFilter {
    pub predicate: String,
    pub value: String,
    pub value_type: String,
    pub r#match: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchOccurrencesRequestPage {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}
