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
    RegisterResponse
},
    service::{
        AuthService,
        AuthServiceError
},
};

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
                (StatusCode::BAD_REQUEST, "invalid email").into_response()
            }
        }
    }
}
