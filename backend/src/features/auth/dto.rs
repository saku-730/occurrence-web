use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse{
    pub message: String,
    pub email: String,
}