use axum::{
    http::StatusCode,
    response::{
        IntoResponse,
        Response,
    },
    Json,
    extract::State,
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

use crate::state::AppState;


#[derive(Debug)]
pub enum AuthHandlerError{
    InvalidEmail,
    Database(sqlx::Error),
}

impl From<AuthServiceError> for AuthHandlerError{
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidEmail => Self::InvalidEmail,
            AuthServiceError::Database(error) => Self::Database(error),
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

                (StatusCode::BAD_REQUEST, Json(body)).into_response() //400

            }
            AuthHandlerError::Database(_) => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response() //500
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/auth/pre_register",
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
        ),
        (
            status = 500,
            description = "Internal server error",
            body = ErrorResponse
        )
    ),
    tag = "auth"
)]
pub async fn pre_register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AuthHandlerError> {
    let output = AuthService::pre_register(
        &state.posgre,
        &state.config.app.app_base_url,
        payload.email,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(output.response)))
}