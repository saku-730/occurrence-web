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
}

impl From<AuthServiceError> for AuthHandlerError{ //.await?用
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidEmail => Self::InvalidEmail,
            AuthServiceError::Database(error) => Self::Database(error),
            AuthServiceError::InvalidToken => Self::InvalidToken,
            AuthServiceError::InvalidPassword => Self::InvalidPassword,
            AuthServiceError::InvalidUserName => Self::InvalidUserName,
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