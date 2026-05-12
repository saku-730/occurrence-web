use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub email: String,
}

#[derive(Debug, Clone,Serialize, ToSchema,PartialEq,Eq)]
pub struct RegisterResponse{
    pub message: String,
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse{
    pub error: String,
    pub message: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteRegistrationRequest {
    pub token: String,
    pub user_name: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CompleteRegistrationResponse {
    pub message: String,
}