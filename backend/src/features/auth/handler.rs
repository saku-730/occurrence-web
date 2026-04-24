use axum::{
    http::StatusCode,
    response::{
        IntoResponse,
        Response,
    },
    Json,
};

use super::{
    dto::{
    RegisterRequest,
    RegisterResponse,
    ErrorResponse
},
    service::{
        AuthService,
        AuthServiceError
},
};

#[utoipa::path(
    post,
    path = "/auth/register",
    request_body = RegisterRequest,
    responses(
        (
            status = 201,
            description = "Register user successfully",
            body = RegisterResponse
        ),
        (
            status = 400,
            description = "Invalid email",
            body = ErrorResponse
        )
    ),
    tag = "auth"
)]
pub async fn register(
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AuthHandlerError> {
    let response = AuthService::register(payload.email).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

#[derive(Debug)]
pub enum AuthHandlerError {
    InvalidEmail
}

impl From<AuthServiceError> for AuthHandlerError {
    fn from(value: AuthServiceError) -> Self {
        match value {
            AuthServiceError::InvalidEmail => AuthHandlerError::InvalidEmail,
        }
    }
}

impl IntoResponse for AuthHandlerError {
    fn into_response(self) -> Response {
        match self {
            AuthHandlerError::InvalidEmail => {
                let body = ErrorResponse {
                    error: "invalid_email".to_string(),
                    message: "Invalid email".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
        }
    }
}
