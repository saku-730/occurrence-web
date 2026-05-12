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
    ErrorResponse,
    CompleteRegistrationRequest,
    CompleteRegistrationResponse,
    LoginRequest,
    LoginResponse,
},
    service::{
        AuthService,
        AuthServiceError
},
    mail::{send_mail, MailError},
};

use crate::state::AppState;

#[derive(Debug)]
pub enum AuthHandlerError{
    InvalidEmail,
    Database(sqlx::Error),
    Mail(MailError),   
    InvalidToken,
    InvalidPassword,
    InvalidUserName,
    PasswordHash,
    EmailAlreadyRegistered,
    InvalidCredentials,
}

impl From<AuthServiceError> for AuthHandlerError{ //.await?用
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidEmail => Self::InvalidEmail,
            AuthServiceError::Database(error) => Self::Database(error),
            AuthServiceError::InvalidToken => Self::InvalidToken,
            AuthServiceError::InvalidPassword => Self::InvalidPassword,
            AuthServiceError::InvalidUserName => Self::InvalidUserName,
            AuthServiceError::PasswordHash => Self::PasswordHash,
            AuthServiceError::EmailAlreadyRegistered => Self::EmailAlreadyRegistered,
            AuthServiceError::InvalidCredentials => Self::InvalidCredentials,
        }
    }
}

impl From<MailError> for AuthHandlerError { //.await?用
    fn from(error: MailError) -> Self {
        Self::Mail(error)        
    }
}

impl IntoResponse for AuthHandlerError { //エラーをhttpレスポンスに変換 axumのやつ
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

            AuthHandlerError::Mail(_) => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }

            AuthHandlerError::InvalidToken => { 
                let body = ErrorResponse {
                    error: "invalid_token".to_string(),
                    message: "Invalid token".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response() //400
            }

            AuthHandlerError::InvalidPassword => { 
                let body = ErrorResponse {
                    error: "invalid_password".to_string(),
                    message: "Invalid password".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response() //400
            }

            AuthHandlerError::InvalidUserName => { 
                let body = ErrorResponse {
                    error: "invalid_username".to_string(),
                    message: "Invalid username".to_string(),
                };

                (StatusCode::BAD_REQUEST, Json(body)).into_response() //400
            }

            AuthHandlerError::PasswordHash => { 
                let body = ErrorResponse {
                    error: "invalid_server_error".to_string(),
                    message: "Invalid server_error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response() //400
            }
            AuthHandlerError::EmailAlreadyRegistered => { 
                let body = ErrorResponse {
                    error: "email_already_registered".to_string(),
                    message: "Email already registered".to_string(),
                };

                (StatusCode::CONFLICT, Json(body)).into_response() //400
            }
            AuthHandlerError::InvalidCredentials => { 
                let body = ErrorResponse {
                    error: "invalid_credentials".to_string(),
                    message: "Invalid credential".to_string(),
                };

                (StatusCode::UNAUTHORIZED, Json(body)).into_response() //400
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

    send_mail(&output.mail, &state.config.smtp).await?; //メール送信

    Ok((StatusCode::CREATED, Json(output.response)))
}

#[utoipa::path(
    post,
    path = "/auth/complete_registration",
    request_body = CompleteRegistrationRequest,
    responses(
        (
            status = 201,
            description = "Complete registration successfully",
            body = CompleteRegistrationResponse
        ),
        (
            status = 400,
            description = "Invalid registration input or token",
            body = ErrorResponse
        ),
        (
            status = 409,
            description = "Email already registered",
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

pub async fn complete_registration(
    State(state): State<AppState>,
    Json(payload): Json<CompleteRegistrationRequest>,
    ) -> Result<(StatusCode, Json<CompleteRegistrationResponse>), AuthHandlerError> {
    AuthService::complete_registration(
        &state.posgre,
        payload.token,
        payload.user_name,
        payload.password,
    )
    .await?;

    let response = CompleteRegistrationResponse {
        message: "registration completed".to_string(),
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<(StatusCode, Json<LoginResponse>), AuthHandlerError> {
    let output = AuthService::login(
        &state.posgre,
        payload.email,
        payload.password,
    )
    .await?;

    let response = LoginResponse {
        message: "login successful".to_string(),
        email: output.email,
        user_name: output.user_name,
    };

    Ok((StatusCode::OK, Json(response)))
}