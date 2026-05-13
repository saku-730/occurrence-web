use utoipa::OpenApi;

use crate::features::auth::dto::{
    CompleteRegistrationRequest,
    CompleteRegistrationResponse,
    RegisterRequest, 
    RegisterResponse,
    ErrorResponse,
    LoginRequest,
    LoginResponse,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::auth::handler::pre_register,
        crate::features::auth::handler::complete_registration,
        crate::features::auth::handler::login,
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
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints")
    )
)]
pub struct ApiDoc;