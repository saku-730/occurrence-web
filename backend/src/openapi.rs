use utoipa::OpenApi;

use crate::features::auth::dto::{
    CompleteRegistrationRequest,
    CompleteRegistrationResponse,
    RegisterRequest, 
    RegisterResponse,
    ErrorResponse,
    LoginRequest,
    LoginResponse,
    LogoutResponse,
    CurrentUserResponse,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::auth::handler::pre_register,
        crate::features::auth::handler::complete_registration,
        crate::features::auth::handler::login,
        crate::features::auth::handler::logout,
        crate::features::auth::handler::me,
    ),
    components(
        schemas(
            RegisterRequest, 
            RegisterResponse, 
            ErrorResponse,
            CompleteRegistrationRequest,
            CompleteRegistrationResponse,
            LoginRequest,
            LoginResponse,
            LogoutResponse, 
            CurrentUserResponse,
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints")
    )
)]
pub struct ApiDoc;