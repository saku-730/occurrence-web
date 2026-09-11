use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header::COOKIE},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::{
    features::{
        auth::{
            dto::ErrorResponse,
            service::{AuthService, AuthServiceError},
        },
        label_templates::{
            dto::{
                DeleteLabelTemplateResponse, LabelTemplateResponse, ListLabelTemplatesResponse,
                SaveLabelTemplateRequest,
            },
            service::{LabelTemplateService, LabelTemplateServiceError},
        },
    },
    state::AppState,
};

#[derive(Debug)]
pub enum LabelTemplateHandlerError {
    InvalidSession,
    InvalidTemplate(String),
    LimitReached,
    NotFound,
    Database(sqlx::Error),
    Internal,
}

impl From<AuthServiceError> for LabelTemplateHandlerError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::Database(error) => Self::Database(error),
            _ => Self::InvalidSession,
        }
    }
}

impl From<LabelTemplateServiceError> for LabelTemplateHandlerError {
    fn from(error: LabelTemplateServiceError) -> Self {
        match error {
            LabelTemplateServiceError::Validation(message) => Self::InvalidTemplate(message),
            LabelTemplateServiceError::LimitReached => Self::LimitReached,
            LabelTemplateServiceError::NotFound => Self::NotFound,
            LabelTemplateServiceError::Database(error) => Self::Database(error),
            LabelTemplateServiceError::StoredTemplateInvalid
            | LabelTemplateServiceError::Serialization => Self::Internal,
        }
    }
}

impl IntoResponse for LabelTemplateHandlerError {
    fn into_response(self) -> Response {
        match self {
            Self::InvalidSession => error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_session",
                "Invalid session",
            ),
            Self::InvalidTemplate(message) => error_response(
                StatusCode::BAD_REQUEST,
                "invalid_label_template",
                &message,
            ),
            Self::LimitReached => error_response(
                StatusCode::CONFLICT,
                "label_template_limit_reached",
                "A user can save up to 10 label templates",
            ),
            Self::NotFound => error_response(
                StatusCode::NOT_FOUND,
                "label_template_not_found",
                "Label template not found",
            ),
            Self::Database(error) => {
                eprintln!("label template database operation failed: {error}");
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_server_error",
                    "Internal server error",
                )
            }
            Self::Internal => error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_server_error",
                "Internal server error",
            ),
        }
    }
}

#[utoipa::path(
    post,
    path = "/label-templates",
    request_body = SaveLabelTemplateRequest,
    responses(
        (status = 201, description = "Label template created", body = LabelTemplateResponse),
        (status = 400, description = "Invalid label template", body = ErrorResponse),
        (status = 401, description = "Login required", body = ErrorResponse),
        (status = 409, description = "Per-user label template limit reached", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "label-templates"
)]
pub async fn create_label_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<SaveLabelTemplateRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<LabelTemplateResponse>), LabelTemplateHandlerError> {
    let user_id = authenticated_user_id(&state, &headers).await?;
    let request = parse_request(payload)?;
    let response = LabelTemplateService::create(&state.posgre, user_id, request.template).await?;

    Ok((StatusCode::CREATED, Json(response)))
}

#[utoipa::path(
    get,
    path = "/label-templates",
    responses(
        (status = 200, description = "Current user's label templates", body = ListLabelTemplatesResponse),
        (status = 401, description = "Login required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "label-templates"
)]
pub async fn list_label_templates(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ListLabelTemplatesResponse>, LabelTemplateHandlerError> {
    let user_id = authenticated_user_id(&state, &headers).await?;
    let response = LabelTemplateService::list(&state.posgre, user_id).await?;

    Ok(Json(response))
}

#[utoipa::path(
    get,
    path = "/label-templates/{template_id}",
    params(("template_id" = Uuid, Path, description = "Label template UUID")),
    responses(
        (status = 200, description = "Label template", body = LabelTemplateResponse),
        (status = 401, description = "Login required", body = ErrorResponse),
        (status = 404, description = "Template does not exist or belongs to another user", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "label-templates"
)]
pub async fn get_label_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(template_id): Path<Uuid>,
) -> Result<Json<LabelTemplateResponse>, LabelTemplateHandlerError> {
    let user_id = authenticated_user_id(&state, &headers).await?;
    let response = LabelTemplateService::get(&state.posgre, user_id, template_id).await?;

    Ok(Json(response))
}

#[utoipa::path(
    put,
    path = "/label-templates/{template_id}",
    params(("template_id" = Uuid, Path, description = "Label template UUID")),
    request_body = SaveLabelTemplateRequest,
    responses(
        (status = 200, description = "Label template updated", body = LabelTemplateResponse),
        (status = 400, description = "Invalid label template", body = ErrorResponse),
        (status = 401, description = "Login required", body = ErrorResponse),
        (status = 404, description = "Template does not exist or belongs to another user", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "label-templates"
)]
pub async fn update_label_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(template_id): Path<Uuid>,
    payload: Result<Json<SaveLabelTemplateRequest>, JsonRejection>,
) -> Result<Json<LabelTemplateResponse>, LabelTemplateHandlerError> {
    let user_id = authenticated_user_id(&state, &headers).await?;
    let request = parse_request(payload)?;
    let response =
        LabelTemplateService::update(&state.posgre, user_id, template_id, request.template).await?;

    Ok(Json(response))
}

#[utoipa::path(
    delete,
    path = "/label-templates/{template_id}",
    params(("template_id" = Uuid, Path, description = "Label template UUID")),
    responses(
        (status = 200, description = "Label template deleted", body = DeleteLabelTemplateResponse),
        (status = 401, description = "Login required", body = ErrorResponse),
        (status = 404, description = "Template does not exist or belongs to another user", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "label-templates"
)]
pub async fn delete_label_template(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(template_id): Path<Uuid>,
) -> Result<Json<DeleteLabelTemplateResponse>, LabelTemplateHandlerError> {
    let user_id = authenticated_user_id(&state, &headers).await?;
    LabelTemplateService::delete(&state.posgre, user_id, template_id).await?;

    Ok(Json(DeleteLabelTemplateResponse { deleted: true }))
}

async fn authenticated_user_id(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Uuid, LabelTemplateHandlerError> {
    let session_token = extract_session_token(headers)?;
    let current_user = AuthService::current_user(&state.posgre, session_token).await?;
    Ok(current_user.user_id)
}

fn parse_request(
    payload: Result<Json<SaveLabelTemplateRequest>, JsonRejection>,
) -> Result<SaveLabelTemplateRequest, LabelTemplateHandlerError> {
    payload
        .map(|Json(request)| request)
        .map_err(|_| LabelTemplateHandlerError::InvalidTemplate("Invalid request body".to_string()))
}

fn extract_session_token(headers: &HeaderMap) -> Result<String, LabelTemplateHandlerError> {
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(LabelTemplateHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| LabelTemplateHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(token) = cookie.strip_prefix("session=") {
            if token.trim().is_empty() {
                return Err(LabelTemplateHandlerError::InvalidSession);
            }
            return Ok(token.to_string());
        }
    }

    Err(LabelTemplateHandlerError::InvalidSession)
}

fn error_response(status: StatusCode, error: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            error: error.to_string(),
            message: message.to_string(),
        }),
    )
        .into_response()
}
