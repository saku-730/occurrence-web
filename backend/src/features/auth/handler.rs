use axum::http::{
    HeaderMap, HeaderValue, StatusCode,
    header::{COOKIE, SET_COOKIE},
};
use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};

use super::{
    dto::{
        CompleteRegistrationRequest, CompleteRegistrationResponse, CurrentUserResponse,
        ErrorResponse, LoginRequest, LoginResponse, LogoutResponse, PasswordResetRequest,
        PasswordResetResponse, RegisterRequest, RegisterResponse,
    },
    mail::{MailError, send_mail},
    service::{AuthService, AuthServiceError},
};

use crate::state::AppState;

#[derive(Debug)]
pub enum AuthHandlerError {
    InvalidEmail,
    Database(sqlx::Error),
    Mail(MailError),
    InvalidToken,
    InvalidPassword,
    InvalidUserName,
    PasswordHash,
    EmailAlreadyRegistered,
    InvalidCredentials,
    InvalidSessionCookie,
    InvalidSession,
}

impl From<AuthServiceError> for AuthHandlerError {
    //.await?用
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
            AuthServiceError::InvalidSession => Self::InvalidSession,
        }
    }
}

impl From<MailError> for AuthHandlerError {
    //.await?用
    fn from(error: MailError) -> Self {
        Self::Mail(error)
    }
}

// service/repositoryの失敗をHTTP APIのエラー形式に変換する境界。
impl IntoResponse for AuthHandlerError {
    //エラーをhttpレスポンスに変換 axumのやつ
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

            AuthHandlerError::InvalidSessionCookie => {
                let body = ErrorResponse {
                    error: "internal_server_error".to_string(),
                    message: "Internal server error".to_string(),
                };

                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }

            AuthHandlerError::InvalidSession => {
                let body = ErrorResponse {
                    error: "invalid_session".to_string(),
                    message: "Invalid session".to_string(),
                };

                (StatusCode::UNAUTHORIZED, Json(body)).into_response()
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
    let output =
        AuthService::pre_register(&state.posgre, &state.config.app.app_base_url, payload.email)
            .await?;

    // 仮登録はメール送信まで成功して初めて完了扱いにする。送信失敗時はHTTPエラーにする。
    send_mail(&output.mail, &state.config.smtp).await?; //メール送信

    Ok((StatusCode::CREATED, Json(output.response)))
}

#[utoipa::path(
    post,
    path = "/auth/request_password_reset",
    request_body = PasswordResetRequest,
    responses(
        (
            status = 200,
            description = "Password reset mail accepted",
            body = PasswordResetResponse
        ),
        (
            status = 400,
            description = "Invalid email",
            body = ErrorResponse
        ),
        (
            status = 401,
            description = "Unknown email",
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
pub async fn request_password_reset(
    State(state): State<AppState>,
    Json(payload): Json<PasswordResetRequest>,
) -> Result<(StatusCode, Json<PasswordResetResponse>), AuthHandlerError> {
    let output = AuthService::request_password_reset(
        &state.posgre,
        &state.config.app.app_base_url,
        payload.email,
    )
    .await?;

    // リセットtokenのDB保存だけで成功扱いにするとユーザーはメールを受け取れない。
    // pre_registerと同様、メール送信まで完了してからHTTP 200を返す。
    send_mail(&output.mail, &state.config.smtp).await?;

    Ok((
        StatusCode::OK,
        Json(PasswordResetResponse {
            message: "password reset mail sent".to_string(),
        }),
    ))
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

#[utoipa::path(
    post,
    path = "/auth/login",
    request_body = LoginRequest,
    responses(
        (
            status = 200,
            description = "Login successfully",
            body = LoginResponse
        ),
        (
            status = 401,
            description = "Invalid credential",
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

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<(StatusCode, HeaderMap, Json<LoginResponse>), AuthHandlerError> {
    let output = AuthService::login(&state.posgre, payload.email, payload.password).await?;

    // Secure属性は本番HTTPSでのみcookieを送るための設定。開発HTTPではconfigで無効にできる。
    let secure_attribute = if state.config.app.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let session_cookie = format!(
        "session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=604800{}", //Max-Ageは秒数 7日
        output.session_token, secure_attribute
    );

    let mut headers = HeaderMap::new();

    headers.insert(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie)
            .map_err(|_| AuthHandlerError::InvalidSessionCookie)?,
    );

    let response = LoginResponse {
        message: "login successful".to_string(),
        email: output.email,
        user_name: output.user_name,
    };

    Ok((StatusCode::OK, headers, Json(response)))
}

#[utoipa::path(
    post,
    path = "/auth/logout",
    responses(
        (
            status = 200,
            description = "Logout successfully",
            body = LogoutResponse
        ),
        (
            status = 401,
            description = "Invalid or missing session",
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

pub async fn logout(
    // path /auth/logout
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, HeaderMap, Json<LogoutResponse>), AuthHandlerError> {
    let session_token = extract_session_token(&headers)?;

    AuthService::logout(&state.posgre, session_token).await?;

    let mut response_headers = HeaderMap::new();

    // logout時もlogin時と同じ属性でcookieを消す。属性がずれるとブラウザに残る可能性がある。
    let secure_attribute = if state.config.app.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    let clear_session_cookie = format!(
        "session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{}",
        secure_attribute
    );

    response_headers.insert(
        //http レスポンスヘッダー作成
        SET_COOKIE,
        HeaderValue::from_str(&clear_session_cookie)
            .map_err(|_| AuthHandlerError::InvalidSessionCookie)?,
    );

    let response = LogoutResponse {
        message: "logout successful".to_string(),
    };

    Ok((StatusCode::OK, response_headers, Json(response)))
}

#[utoipa::path(
    get,
    path = "/auth/me",
    responses(
        (
            status = 200,
            description = "Get current authenticated user",
            body = CurrentUserResponse
        ),
        (
            status = 401,
            description = "Invalid or missing session",
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

pub async fn me(
    // path /auth/me
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<CurrentUserResponse>), AuthHandlerError> {
    let session_token = extract_session_token(&headers)?; //トークン取り出し

    let output = AuthService::current_user(&state.posgre, session_token).await?;

    let response = CurrentUserResponse {
        //httpレスポンス組み立て
        user_id: output.user_id,
        email: output.email,
        user_name: output.user_name,
        role: output.role,
    };

    Ok((StatusCode::OK, Json(response)))
}

// Cookieヘッダーは複数cookieを1行で送るため、session=だけを安全に取り出す。
fn extract_session_token(headers: &HeaderMap) -> Result<String, AuthHandlerError> {
    //セッションcookieを取り出す。
    let cookie_header = headers
        .get(COOKIE)
        .ok_or(AuthHandlerError::InvalidSession)?
        .to_str()
        .map_err(|_| AuthHandlerError::InvalidSession)?;

    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();

        if let Some(session_token) = cookie.strip_prefix("session=") {
            if session_token.trim().is_empty() {
                return Err(AuthHandlerError::InvalidSession);
            }

            return Ok(session_token.to_string()); //取り出せたらここで終わり
        }
    }

    Err(AuthHandlerError::InvalidSession)
}
