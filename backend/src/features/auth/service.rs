use super::dto::RegisterResponse;
use super::repository::AuthRepository;
use crate::features::auth::mail::{build_registration_completion_email, MailMessage};

use email_address::EmailAddress;
use sha2::{Digest,Sha256};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug)]
pub enum AuthServiceError {
    InvalidEmail,
    Database(sqlx::Error),
    InvalidToken, //トークンエラー トークンが空とか
    InvalidPassword, //パスワードが空、空白だけ
    InvalidUserName, //ユーザー名が空か空白だけ
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreRegisterOutput {
    pub response: RegisterResponse,
    pub mail: MailMessage,
}

impl From<sqlx::Error> for AuthServiceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

pub struct AuthService;//とりあえずメソッド用に作っておく。

impl AuthService {
    pub async fn pre_register(
        db: &PgPool,  
        app_base_url: &str, 
        email: String,
    ) -> Result<PreRegisterOutput, AuthServiceError> {
        let email = email.trim().to_lowercase();//前後空白を削除&小文字化

        if !EmailAddress::is_valid(&email){ //メールアドレスのvalidation
            return Err(AuthServiceError::InvalidEmail);
        }

        let token = Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);

        AuthRepository::create_pending_registration(db, &email, &token_hash).await?; //データベース書き込み
                                                                                     //
        let response = RegisterResponse {
            message: "temporary registration accepted".to_string(),
            email: email.clone(),
        };

        let mail = build_registration_completion_email(&email, app_base_url, &token);

        Ok(PreRegisterOutput{ response, mail })
    }
    pub async fn complete_registration( //登録を完了するための関数
    db: &PgPool,
    token: String,
    user_name: String,
    password: String,
    ) -> Result<(), AuthServiceError> {
    let token = token.trim();

    if token.is_empty() {
        return Err(AuthServiceError::InvalidToken); //トークンが空はエラー
    }

    if password.trim().is_empty() {
        return Err(AuthServiceError::InvalidPassword);
    }

    if user_name.trim().is_empty() {
        return Err(AuthServiceError::InvalidUserName);
    }
    
    let token_hash = hash_token(token);

    let pending_registration = AuthRepository::find_pending_registration_by_token_hash(db, &token_hash).await?;

    if pending_registration.is_none() {
        return Err(AuthServiceError::InvalidToken);
    }

    Ok(())
    }
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes())) //ハッシュ化, encodeはバイナリそのままを16進数に変換している。
}




#[cfg(test)]
mod tests {

    use super::{AuthService, AuthServiceError};
    use sqlx::{postgres::PgPoolOptions, PgPool};

    async fn test_db_pool() -> PgPool {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for DB tests");

        PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("failed to connect test database")
    }

    async fn delete_pending_registration_by_email(db: &PgPool, email: &str) {
        sqlx::query(
            r#"
            DELETE FROM pending_registrations
            WHERE email = $1
            "#,
        )
        .bind(email)
        .execute(db)
        .await
        .expect("failed to delete pending registration");
    }

    async fn count_pending_registration_by_email(db: &PgPool, email: &str) -> i64 {
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM pending_registrations
            WHERE email = $1
            "#,
        )
        .bind(email)
        .fetch_one(db)
        .await
        .expect("failed to count pending registration");

        row.0
    }

    async fn get_token_hash_by_email(db: &PgPool, email: &str) -> String {
        let row: (String,) = sqlx::query_as(
            r#"
            SELECT token_hash
            FROM pending_registrations
            WHERE email = $1
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(email)
        .fetch_one(db)
        .await
        .expect("failed to fetch token_hash");

        row.0
    }

    #[tokio::test]
    async fn pre_register_accepts_valid_email_and_creates_pending_registration() {
        let db = test_db_pool().await;

        let app_base_url = "http://127.0.0.1:3000";
        let email = format!("service-valid-{}@example.com", uuid::Uuid::new_v4());


        //delete_pending_registration_by_email(&db, email).await;

        let result = AuthService::pre_register(&db,app_base_url, email.to_string()).await;

        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.response.email, email);
        assert_eq!(response.response.message, "temporary registration accepted");

        let count = count_pending_registration_by_email(&db, &email).await;
        assert_eq!(count, 1);

        delete_pending_registration_by_email(&db, &email).await;
    }

    #[tokio::test]
    async fn pre_register_trims_and_lowercases_email_and_creates_pending_registration() {
        let db = test_db_pool().await;
        let normalized_email = "test@example.com";
        let app_base_url = "http://127.0.0.1:3000";

        delete_pending_registration_by_email(&db, normalized_email).await;

        let result = AuthService::pre_register(&db, app_base_url,  "  TEST@EXAMPLE.COM  ".to_string()).await;

        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.response.email, normalized_email);

        let count = count_pending_registration_by_email(&db, normalized_email).await;
        assert_eq!(count, 1);

        delete_pending_registration_by_email(&db, normalized_email).await;
    }

    #[tokio::test]
    async fn pre_register_rejects_empty_email() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result = AuthService::pre_register(&db, app_base_url,"   ".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_without_at() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result = AuthService::pre_register(&db, app_base_url,  "invalid-email".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_without_local_part() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result = AuthService::pre_register(&db, app_base_url,"@example.com".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_without_domain_part() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result = AuthService::pre_register(&db, app_base_url,  "test@".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_with_multiple_at_marks() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result = AuthService::pre_register(&db, app_base_url, "test@@example.com".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_stores_token_hash() {
        let db = test_db_pool().await;
        let email = "service-token-hash@example.com";
        let app_base_url = "http://127.0.0.1:3000";

        delete_pending_registration_by_email(&db, email).await;

        let result = AuthService::pre_register(&db,app_base_url,email.to_string()).await;

        assert!(result.is_ok());

        let token_hash = get_token_hash_by_email(&db, email).await;

        assert_eq!(token_hash.len(), 64);
        assert!(token_hash.chars().all(|c| c.is_ascii_hexdigit()));

        delete_pending_registration_by_email(&db, email).await;
    }

    #[tokio::test]
    async fn pre_register_rejects_invalid_email_and_does_not_create_pending_registration() {
        let db = test_db_pool().await;
        let invalid_email = format!("invalid-{}", uuid::Uuid::new_v4());
        let app_base_url = "http://127.0.0.1:3000";

        delete_pending_registration_by_email(&db, &invalid_email).await;

        let result = AuthService::pre_register(&db, app_base_url,invalid_email.clone()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));

        let count = count_pending_registration_by_email(&db, &invalid_email).await;
        assert_eq!(count, 0);

        delete_pending_registration_by_email(&db, &invalid_email).await;
    }

    #[tokio::test]
    async fn pre_register_creates_registration_completion_email() {
        let db = test_db_pool().await;
        let email = format!("service-mail-{}@example.com", uuid::Uuid::new_v4());
        let app_base_url = "http://127.0.0.1:3000";

        delete_pending_registration_by_email(&db, &email).await;

        let result = AuthService::pre_register(&db, app_base_url, email.clone()).await;

        assert!(result.is_ok());

        let output = result.unwrap();

        assert_eq!(output.response.email, email);
        assert_eq!(output.response.message, "temporary registration accepted");

        assert_eq!(output.mail.to, email);
        assert!(output.mail.subject.contains("registration"));
        assert!(
            output
                .mail
                .body
                .contains("http://127.0.0.1:3000/auth/complete_registration")
        );
        assert!(output.mail.body.contains("token="));

        delete_pending_registration_by_email(&db, &email).await;
    }

    #[tokio::test]
    async fn complete_registration_rejects_empty_token() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "".to_string(),
            "saku".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(matches!(
            result,
            Err(AuthServiceError::InvalidToken)
        ));
    }
    
    #[tokio::test]
    async fn complete_registration_rejects_unknown_token() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "unknown-token".to_string(),
            "saku".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(matches!(
            result,
            Err(AuthServiceError::InvalidToken)
        ));
    }

    #[tokio::test]
    async fn complete_registration_rejects_empty_password() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "test-token".to_string(),
            "saku".to_string(),
            "".to_string(),
        )
        .await;

        assert!(matches!(
            result,
            Err(AuthServiceError::InvalidPassword)
        ));
    }

    #[tokio::test]
    async fn complete_registration_rejects_blank_password() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "test-token".to_string(),
            "saku".to_string(),
            "   ".to_string(),
        )
        .await;

        assert!(matches!(
            result,
            Err(AuthServiceError::InvalidPassword)
        ));
    }

    #[tokio::test]
    async fn complete_registration_rejects_empty_user_name() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "test-token".to_string(),
            "".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(matches!(
            result,
            Err(AuthServiceError::InvalidUserName)
        ));
    }

    #[tokio::test]
    async fn complete_registration_rejects_blank_user_name() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "test-token".to_string(),
            "   ".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(matches!(
            result,
            Err(AuthServiceError::InvalidUserName)
        ));
    }
}