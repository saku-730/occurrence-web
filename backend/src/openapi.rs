use utoipa::OpenApi;

use crate::features::auth::dto::{
    CompleteRegistrationRequest,
    CompleteRegistrationResponse,
    RegisterRequest, 
    RegisterResponse,
    ErrorResponse
};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::auth::handler::pre_register,
        crate::features::auth::handler::complete_registration
    ),
    components(
        schemas(
            RegisterRequest, 
            RegisterResponse, 
            ErrorResponse,
            CompleteRegistrationRequest,
            CompleteRegistrationResponse,
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints")
    )
)]
pub struct ApiDoc;