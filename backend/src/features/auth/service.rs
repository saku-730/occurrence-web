use super::dto::RegisterResponse;

#[derive(Debug)]
pub enum AuthServiceError {
    InvalidEmail,
}

pub struct AuthService;

impl AuthService {
    pub async fn register(email: String) 
    -> Result<RegisterResponse, AuthServiceError> {
        let email = email.trim().to_lowercase();

        if email.is_empty() || !email.contains('@'){
            return Err(AuthServiceError::InvalidEmail);
        }

        Ok(RegisterResponse{
            message: "temporary registration accepted".to_string(),
            email,
        })
    }
}


#[cfg(test)]
mod tests {
    use super::{AuthService, AuthServiceError};

    #[tokio::test]
    async fn register_accepts_valid_email() {
        let result = AuthService::register("test@example.com".to_string()).await;

        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.email, "test@example.com");
        assert_eq!(response.message, "temporary registration accepted");
    }

    #[tokio::test]
    async fn register_trims_and_lowercases_email() {
        let result = AuthService::register("  TEST@EXAMPLE.COM  ".to_string()).await;

        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.email, "test@example.com");
    }

    #[tokio::test]
    async fn register_rejects_empty_email() {
        let result = AuthService::register("   ".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn register_rejects_email_without_at() {
        let result = AuthService::register("invalid-email".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }
}