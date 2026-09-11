use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabelTemplateDefinition {
    pub version: u32,
    pub name: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub font_size_mm: f64,
    pub qr: LabelTemplateQr,
    pub fields: Vec<LabelTemplateField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabelTemplateQr {
    pub enabled: bool,
    pub size_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabelTemplateField {
    #[serde(rename = "type")]
    pub field_type: LabelTemplateFieldType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LabelTemplateFieldType {
    Builtin,
    DarwinCore,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveLabelTemplateRequest {
    pub template: LabelTemplateDefinition,
}

#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct LabelTemplateResponse {
    pub id: Uuid,
    pub template: LabelTemplateDefinition,
}

#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct ListLabelTemplatesResponse {
    pub templates: Vec<LabelTemplateResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema, PartialEq, Eq)]
pub struct DeleteLabelTemplateResponse {
    pub deleted: bool,
}
