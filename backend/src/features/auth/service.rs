use super::dto::RegisterResponse;

use email_address::EmailAddress;

#[derive(Debug)]
pub enum AuthServiceError {
    InvalidEmail,
}

pub struct AuthService;//とりあえずメソッド用に作っておく。

impl AuthService {
    pub async fn pre_register(email: String) 
    -> Result<RegisterResponse, AuthServiceError> {
        let email = email.trim().to_lowercase();//前後空白を削除&小文字化

        if !EmailAddress::is_valid(&email){ //メールアドレスのvalidation
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
        let result = AuthService::pre_register("test@example.com".to_string()).await;

        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.email, "test@example.com");
        assert_eq!(response.message, "temporary registration accepted");
    }

    #[tokio::test]
    async fn register_trims_and_lowercases_email() {
        let result = AuthService::pre_register("  TEST@EXAMPLE.COM  ".to_string()).await;

        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.email, "test@example.com");
    }

    #[tokio::test]
    async fn register_rejects_empty_email() {
        let result = AuthService::pre_register("   ".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn register_rejects_email_without_at() {
        let result = AuthService::pre_register("invalid-email".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn register_rejects_email_without_local_part() {
        let result = AuthService::pre_register("@example.com".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn register_rejects_email_without_domain_part() {
        let result = AuthService::pre_register("test@".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn register_rejects_email_with_multiple_at_marks() {
        let result = AuthService::pre_register("test@@example.com".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }
}