use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RegisterResponse{
    pub message: String,
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse{
    pub error: String,
    pub message: String,
}