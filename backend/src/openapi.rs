use utoipa::OpenApi;

use crate::features::auth::dto::{RegisterRequest, RegisterResponse,ErrorResponse};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::features::auth::handler::register
    ),
    components(
        schemas(RegisterRequest, RegisterResponse, ErrorResponse)
    ),
    tags(
        (name = "auth", description = "Authentication endpoints")
    )
)]
pub struct ApiDoc;