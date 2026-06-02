use utoipa::OpenApi;

use crate::features::auth::dto::{
    CompleteRegistrationRequest, CompleteRegistrationResponse, CurrentUserResponse, ErrorResponse,
    LoginRequest, LoginResponse, LogoutResponse, RegisterRequest, RegisterResponse,
};

use crate::features::occurrences::dto::CreateOccurrenceResponse;

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::auth::handler::pre_register,
        crate::features::auth::handler::complete_registration,
        crate::features::auth::handler::login,
        crate::features::auth::handler::logout,
        crate::features::auth::handler::me,
        crate::features::occurrences::handler::create_occurrence,
        crate::features::occurrences::handler::get_occurrence,
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
            CreateOccurrenceResponse,
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "occurrences", description = "Occurrence RDF endpoints")
    )
)]
pub struct ApiDoc;
