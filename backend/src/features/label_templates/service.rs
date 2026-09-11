use std::collections::HashSet;

use sqlx::PgPool;
use uuid::Uuid;

use super::{
    dto::{
        LabelTemplateDefinition, LabelTemplateFieldType, LabelTemplateResponse,
        ListLabelTemplatesResponse,
    },
    repository::{LabelTemplateRecord, LabelTemplateRepository},
};

const SUPPORTED_TEMPLATE_VERSION: u32 = 1;
const MAX_TEMPLATE_NAME_CHARS: usize = 100;
const MAX_TEMPLATES_PER_USER: i64 = 10;
const MIN_WIDTH_MM: f64 = 20.0;
const MAX_WIDTH_MM: f64 = 200.0;
const MIN_HEIGHT_MM: f64 = 15.0;
const MAX_HEIGHT_MM: f64 = 277.0;
const MIN_FONT_SIZE_MM: f64 = 1.5;
const MAX_FONT_SIZE_MM: f64 = 6.0;
const MIN_QR_SIZE_MM: f64 = 8.0;
const MAX_QR_SIZE_MM: f64 = 50.0;
const LABEL_INNER_MARGIN_MM: f64 = 2.0;
const ALLOWED_BUILTIN_KEYS: &[&str] = &[
    "scientificName",
    "creator",
    "eventDate",
    "locality",
    "coordinates",
    "created",
];

#[derive(Debug)]
pub enum LabelTemplateServiceError {
    Validation(String),
    LimitReached,
    NotFound,
    Database(sqlx::Error),
    StoredTemplateInvalid,
    Serialization,
}

impl From<sqlx::Error> for LabelTemplateServiceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

pub struct LabelTemplateService;

impl LabelTemplateService {
    pub async fn create(
        db: &PgPool,
        user_id: Uuid,
        mut template: LabelTemplateDefinition,
    ) -> Result<LabelTemplateResponse, LabelTemplateServiceError> {
        normalize_template(&mut template);
        validate_template(&template)?;

        let id = Uuid::new_v4();
        let template_json = serde_json::to_string(&template)
            .map_err(|_| LabelTemplateServiceError::Serialization)?;
        let record = LabelTemplateRepository::create_if_under_limit(
            db,
            id,
            user_id,
            &template_json,
            MAX_TEMPLATES_PER_USER,
        )
        .await?
        .ok_or(LabelTemplateServiceError::LimitReached)?;

        response_from_record(record)
    }

    pub async fn list(
        db: &PgPool,
        user_id: Uuid,
    ) -> Result<ListLabelTemplatesResponse, LabelTemplateServiceError> {
        let records = LabelTemplateRepository::list_by_user(db, user_id).await?;
        let templates = records
            .into_iter()
            .map(response_from_record)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ListLabelTemplatesResponse { templates })
    }

    pub async fn get(
        db: &PgPool,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<LabelTemplateResponse, LabelTemplateServiceError> {
        let record = LabelTemplateRepository::find_by_id_and_user(db, id, user_id)
            .await?
            .ok_or(LabelTemplateServiceError::NotFound)?;

        response_from_record(record)
    }

    pub async fn update(
        db: &PgPool,
        user_id: Uuid,
        id: Uuid,
        mut template: LabelTemplateDefinition,
    ) -> Result<LabelTemplateResponse, LabelTemplateServiceError> {
        normalize_template(&mut template);
        validate_template(&template)?;

        let template_json = serde_json::to_string(&template)
            .map_err(|_| LabelTemplateServiceError::Serialization)?;
        let record = LabelTemplateRepository::update(db, id, user_id, &template_json)
            .await?
            .ok_or(LabelTemplateServiceError::NotFound)?;

        response_from_record(record)
    }

    pub async fn delete(
        db: &PgPool,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<(), LabelTemplateServiceError> {
        if LabelTemplateRepository::delete(db, id, user_id).await? {
            Ok(())
        } else {
            Err(LabelTemplateServiceError::NotFound)
        }
    }
}

fn normalize_template(template: &mut LabelTemplateDefinition) {
    template.name = template.name.trim().to_string();
}

fn response_from_record(
    record: LabelTemplateRecord,
) -> Result<LabelTemplateResponse, LabelTemplateServiceError> {
    let template: LabelTemplateDefinition = serde_json::from_str(&record.template_json)
        .map_err(|_| LabelTemplateServiceError::StoredTemplateInvalid)?;
    validate_template(&template).map_err(|_| LabelTemplateServiceError::StoredTemplateInvalid)?;

    Ok(LabelTemplateResponse {
        id: record.id,
        template,
    })
}

fn validate_template(template: &LabelTemplateDefinition) -> Result<(), LabelTemplateServiceError> {
    if template.version != SUPPORTED_TEMPLATE_VERSION {
        return validation_error(format!(
            "unsupported template version: {}",
            template.version
        ));
    }

    let name_len = template.name.chars().count();
    if name_len == 0 || name_len > MAX_TEMPLATE_NAME_CHARS {
        return validation_error(format!(
            "name must contain 1 to {MAX_TEMPLATE_NAME_CHARS} characters"
        ));
    }

    validate_number("widthMm", template.width_mm, MIN_WIDTH_MM, MAX_WIDTH_MM)?;
    validate_number(
        "heightMm",
        template.height_mm,
        MIN_HEIGHT_MM,
        MAX_HEIGHT_MM,
    )?;
    validate_number(
        "fontSizeMm",
        template.font_size_mm,
        MIN_FONT_SIZE_MM,
        MAX_FONT_SIZE_MM,
    )?;
    validate_number(
        "qr.sizeMm",
        template.qr.size_mm,
        MIN_QR_SIZE_MM,
        MAX_QR_SIZE_MM,
    )?;

    if template.qr.enabled
        && (template.qr.size_mm > template.width_mm - LABEL_INNER_MARGIN_MM
            || template.qr.size_mm > template.height_mm - LABEL_INNER_MARGIN_MM)
    {
        return validation_error("qr.sizeMm must fit inside the label".to_string());
    }

    let mut unique_fields = HashSet::new();
    let mut any_enabled = template.qr.enabled;

    for (index, field) in template.fields.iter().enumerate() {
        any_enabled |= field.enabled;

        let identity = match field.field_type {
            LabelTemplateFieldType::Builtin => {
                if field.uri.is_some() {
                    return validation_error(format!(
                        "fields[{index}].uri is not allowed for a builtin field"
                    ));
                }
                let key = field.key.as_deref().ok_or_else(|| {
                    LabelTemplateServiceError::Validation(format!(
                        "fields[{index}].key is required for a builtin field"
                    ))
                })?;
                if !ALLOWED_BUILTIN_KEYS.contains(&key) {
                    return validation_error(format!(
                        "fields[{index}].key is not an allowed builtin field"
                    ));
                }
                format!("builtin:{key}")
            }
            LabelTemplateFieldType::DarwinCore => {
                if field.key.is_some() {
                    return validation_error(format!(
                        "fields[{index}].key is not allowed for a Darwin Core field"
                    ));
                }
                let uri = field.uri.as_deref().ok_or_else(|| {
                    LabelTemplateServiceError::Validation(format!(
                        "fields[{index}].uri is required for a Darwin Core field"
                    ))
                })?;
                if !is_allowed_darwin_core_uri(uri) {
                    return validation_error(format!(
                        "fields[{index}].uri must be a Darwin Core term URI"
                    ));
                }
                format!("darwinCore:{uri}")
            }
        };

        if !unique_fields.insert(identity) {
            return validation_error(format!("fields[{index}] duplicates another field"));
        }
    }

    if !any_enabled {
        return validation_error("at least one display field must be enabled".to_string());
    }

    Ok(())
}

fn validate_number(
    field: &str,
    value: f64,
    min: f64,
    max: f64,
) -> Result<(), LabelTemplateServiceError> {
    if !value.is_finite() || value < min || value > max {
        return validation_error(format!("{field} must be between {min} and {max}"));
    }
    Ok(())
}

fn is_allowed_darwin_core_uri(uri: &str) -> bool {
    let suffix = uri
        .strip_prefix("http://rs.tdwg.org/dwc/terms/")
        .or_else(|| uri.strip_prefix("https://rs.tdwg.org/dwc/terms/"));

    matches!(
        suffix,
        Some(value)
            if !value.is_empty()
                && value.chars().all(|character| {
                    !character.is_whitespace()
                        && !character.is_control()
                        && !matches!(character, '<' | '>' | '"' | '\'')
                })
    )
}

fn validation_error<T>(message: String) -> Result<T, LabelTemplateServiceError> {
    Err(LabelTemplateServiceError::Validation(message))
}
