use super::dto::RegisterResponse;
use super::repository::AuthRepository;
use crate::features::auth::mail::{
    MailMessage, build_password_reset_email, build_registration_completion_email,
};

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use email_address::EmailAddress;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug)]
pub enum AuthServiceError {
    InvalidEmail,
    Database(sqlx::Error),
    InvalidToken,           //トークンエラー トークンが空とか
    InvalidPassword,        //パスワードが空、空白だけ
    InvalidUserName,        //ユーザー名が空か空白だけ
    PasswordHash,           //ハッシュ化したパスワードのエラー
    EmailAlreadyRegistered, //メールがすでに使われている場合
    InvalidCredentials,     //メールアドレスまたはパスワードが間違いの場合
    InvalidSession,         //セッショントークンが空など
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreRegisterOutput {
    pub response: RegisterResponse,
    pub mail: MailMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordResetOutput {
    pub email: String,
    pub mail: MailMessage,
}

#[derive(Debug)]
pub struct LoginOutput {
    pub email: String,
    pub user_name: String,
    pub session_token: String,
}

#[derive(Debug)]
pub struct CurrentUserOutput {
    pub user_id: uuid::Uuid,
    pub email: String,
    pub user_name: String,
    pub role: String,
}

impl From<sqlx::Error> for AuthServiceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<argon2::password_hash::Error> for AuthServiceError {
    //hash_password用
    fn from(_error: argon2::password_hash::Error) -> Self {
        Self::PasswordHash
    }
}

// 認証の業務ルールを集約する。handlerはHTTP変換、repositoryはSQLに寄せるためここで判断する。
pub struct AuthService; //とりあえずメソッド用に作っておく。

impl AuthService {
    pub async fn pre_register(
        db: &PgPool,
        app_base_url: &str,
        email: String,
    ) -> Result<PreRegisterOutput, AuthServiceError> {
        // emailは同一性判定に使うため、登録前に必ず正規化する。
        let email = email.trim().to_lowercase(); //前後空白を削除&小文字化

        if !EmailAddress::is_valid(&email) {
            //メールアドレスのvalidation
            return Err(AuthServiceError::InvalidEmail);
        }

        // メール本文には生tokenを入れるが、DBにはhashだけ保存する。DB漏洩時に登録URLを再利用されないため。
        let token = Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);

        AuthRepository::create_pending_registration(db, &email, &token_hash).await?; //データベース書き込み
        //
        let response = RegisterResponse {
            message: "temporary registration accepted".to_string(),
            email: email.clone(),
        };

        let mail = build_registration_completion_email(&email, app_base_url, &token);

        Ok(PreRegisterOutput { response, mail })
    }

    pub async fn request_password_reset(
        db: &PgPool,
        app_base_url: &str,
        email: String,
    ) -> Result<PasswordResetOutput, AuthServiceError> {
        // login/pre_registerと同じ正規化を行い、同じメールアドレスを大文字小文字で別扱いしない。
        let email = email.trim().to_lowercase();

        if !EmailAddress::is_valid(&email) {
            return Err(AuthServiceError::InvalidEmail);
        }

        let user = AuthRepository::find_user_by_email(db, &email).await?;
        let user = user.ok_or(AuthServiceError::InvalidCredentials)?;

        // リセットURLには生tokenを入れるが、DBにはhashだけ保存する。
        // これによりDBだけ漏洩しても、有効なリセットURLを復元できない。
        let token = Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);

        AuthRepository::upsert_password_reset_token(db, user.id, &token_hash).await?;

        let mail = build_password_reset_email(&email, app_base_url, &token);

        Ok(PasswordResetOutput { email, mail })
    }

    pub async fn complete_registration(
        //登録を完了するための関数
        db: &PgPool,
        token: String,
        user_name: String,
        password: String,
    ) -> Result<(), AuthServiceError> {
        let token = token.trim();

        if token.is_empty() {
            return Err(AuthServiceError::InvalidToken); //トークンが空はエラー
        }

        let password = password.trim();

        if password.is_empty() {
            return Err(AuthServiceError::InvalidPassword);
        }

        // 仕様上の8-128文字制限は、空白trim後の実際に保存するpasswordに対して判定する。
        let password_len = password.chars().count();
        if !(8..=128).contains(&password_len) {
            return Err(AuthServiceError::InvalidPassword);
        }

        if user_name.trim().is_empty() {
            return Err(AuthServiceError::InvalidUserName);
        }

        let token_hash = hash_token(token);

        // ユーザー作成とpending完了は一体の操作なので、片方だけ成功しないようtransactionでまとめる。
        let mut tx = db.begin().await?;

        let pending_registration =
            AuthRepository::find_pending_registration_by_token_hash_in_tx(&mut tx, &token_hash)
                .await?; //pending_registrationからトークンでemail検索

        let pending_registration = pending_registration.ok_or(AuthServiceError::InvalidToken)?; //pending_registrationの取り出し

        if AuthRepository::user_exists_by_email_in_tx(
            //メールの重複確認
            &mut tx,
            &pending_registration.email,
        )
        .await?
        {
            return Err(AuthServiceError::EmailAlreadyRegistered);
        }

        let user_name = user_name.trim();
        let password_hash = hash_password(password)?;

        AuthRepository::create_user_in_tx(
            //ユーザー本作成処理
            &mut tx,
            &pending_registration.email,
            user_name,
            &password_hash,
        )
        .await?;

        AuthRepository::mark_pending_registration_completed_in_tx(
            //完了処理
            &mut tx,
            &token_hash,
        )
        .await?;

        tx.commit().await?;

        Ok(())
    }

    pub async fn login(
        db: &PgPool,
        email: String,
        password: String,
    ) -> Result<LoginOutput, AuthServiceError> {
        // login時も登録時と同じ正規化を行い、大文字小文字や前後空白で別ユーザー扱いにならないようにする。
        let email = email.trim().to_lowercase(); //メール整形

        if email.is_empty() {
            return Err(AuthServiceError::InvalidCredentials);
        }

        if password.trim().is_empty() {
            return Err(AuthServiceError::InvalidCredentials);
        }

        // 存在しないemailとpassword不一致は同じエラーにする。ユーザー列挙を避けるため。
        let user = AuthRepository::find_user_by_email(db, &email).await?;

        let user = user.ok_or(AuthServiceError::InvalidCredentials)?; //ユーザーが見つからなかったらエラー

        verify_password(&password, &user.password_hash)?;

        // Cookieには生tokenを返し、DBにはhashだけ保存する。session漏洩時の被害面をDB側で広げないため。
        let session_token = Uuid::new_v4().to_string(); //トークン作成
        let session_token_hash = hash_token(&session_token); //トークン　ハッシュ化

        AuthRepository::create_session(
            //データベースにセッション保存
            db,
            user.id,
            &session_token_hash,
        )
        .await?;

        Ok(LoginOutput {
            email: user.email,
            user_name: user.user_name,
            session_token: session_token,
        })
    }

    pub async fn logout(db: &PgPool, session_token: String) -> Result<(), AuthServiceError> {
        let session_token = session_token.trim(); //セッショントークン整形

        if session_token.is_empty() {
            //セッショントークンが空だったらエラー
            return Err(AuthServiceError::InvalidSession);
        }

        let session_token_hash = hash_token(session_token);

        let revoked = AuthRepository::revoke_session_by_token_hash(db, &session_token_hash).await?;

        if !revoked {
            //無効化されていなかったらエラー
            return Err(AuthServiceError::InvalidSession);
        }

        Ok(())
    }

    pub async fn current_user(
        //現在ログイン中のユーザー情報取得
        db: &PgPool,
        session_token: String,
    ) -> Result<CurrentUserOutput, AuthServiceError> {
        let session_token = session_token.trim();

        if session_token.is_empty() {
            return Err(AuthServiceError::InvalidSession);
        }

        let session_token_hash = hash_token(session_token);

        let user = AuthRepository::find_user_by_session_token_hash(
            //ユーザー検索
            db,
            &session_token_hash,
        )
        .await?;

        let user = user.ok_or(AuthServiceError::InvalidSession)?;

        Ok(CurrentUserOutput {
            user_id: user.user_id,
            email: user.email,
            user_name: user.user_name,
            // MVPではrole永続化を後回しにしているためeditor固定。admin対応時はDBのroleを返す。
            role: "editor".to_string(), //とりあえず、編集者権限だけ用意しているので、これで
        })
    }
}

// tokenは照合時に再計算できればよいので、可逆暗号ではなくSHA-256 hashで保存する。
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes())) //ハッシュ化, encodeはバイナリそのままを16進数に変換している。
}

// passwordはArgon2idでsalt付きhash化する。平文保存は禁止。
pub fn hash_password(password: &str) -> Result<String, AuthServiceError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)?
        .to_string();

    Ok(password_hash)
}

fn verify_password(password: &str, password_hash: &str) -> Result<(), AuthServiceError> {
    let parsed_hash =
        PasswordHash::new(password_hash).map_err(|_| AuthServiceError::InvalidCredentials)?;

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| AuthServiceError::InvalidCredentials)?;

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::{AuthService, AuthServiceError};
    use crate::features::auth::repository::AuthRepository;
    use crate::features::auth::service::hash_password;
    use crate::features::auth::service::hash_token;
    use chrono;
    use sqlx::{PgPool, postgres::PgPoolOptions};

    async fn test_db_pool() -> PgPool {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for DB tests");

        let db = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("failed to connect test database");

        sqlx::query!(
            //データベースをきれいにしてから。実データ入っている本番環境ではアウト
            r#"
            TRUNCATE users, pending_registrations, sessions
            RESTART IDENTITY
            CASCADE
            "#
        )
        .execute(&db)
        .await
        .expect("test database should be cleaned");

        db
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

        let result = AuthService::pre_register(&db, app_base_url, email.to_string()).await;

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

        let result =
            AuthService::pre_register(&db, app_base_url, "  TEST@EXAMPLE.COM  ".to_string()).await;

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

        let result = AuthService::pre_register(&db, app_base_url, "   ".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_without_at() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result =
            AuthService::pre_register(&db, app_base_url, "invalid-email".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_without_local_part() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result = AuthService::pre_register(&db, app_base_url, "@example.com".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_without_domain_part() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result = AuthService::pre_register(&db, app_base_url, "test@".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_rejects_email_with_multiple_at_marks() {
        let db = test_db_pool().await;
        let app_base_url = "http://127.0.0.1:3000";

        let result =
            AuthService::pre_register(&db, app_base_url, "test@@example.com".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidEmail)));
    }

    #[tokio::test]
    async fn pre_register_stores_token_hash() {
        let db = test_db_pool().await;
        let email = "service-token-hash@example.com";
        let app_base_url = "http://127.0.0.1:3000";

        delete_pending_registration_by_email(&db, email).await;

        let result = AuthService::pre_register(&db, app_base_url, email.to_string()).await;

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

        let result = AuthService::pre_register(&db, app_base_url, invalid_email.clone()).await;

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
    async fn request_password_reset_creates_reset_email_for_registered_email() {
        let db = test_db_pool().await;
        let email = format!("password-reset-{}@example.com", uuid::Uuid::new_v4());
        let app_base_url = "http://127.0.0.1:3000";
        let password_hash = hash_password("password123").expect("password hash should be created");

        // パスワードリセットは登録済みユーザーだけを対象にする。
        // ここでは送信そのものではなく、DBにtoken hashを保存してMailMessageを組み立てるservice責務を確認する。
        AuthRepository::create_user(&db, &email, "reset-user", &password_hash)
            .await
            .expect("user should be created");

        let result = AuthService::request_password_reset(&db, app_base_url, email.clone()).await;

        assert!(
            result.is_ok(),
            "request_password_reset should prepare reset mail for registered email: {:?}",
            result
        );

        let output = result.unwrap();

        assert_eq!(output.email, email);
        assert_eq!(output.mail.to, email);
        assert!(output.mail.subject.contains("パスワードリセット"));
        assert!(
            output
                .mail
                .body
                .contains("Occurrence Web のパスワードリセットを受け付けました。")
        );
        assert!(
            output
                .mail
                .body
                .contains("http://127.0.0.1:3000/auth/reset_password")
        );
        assert!(output.mail.body.contains("token="));

        let token_hash_count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM password_reset_tokens
            WHERE user_id = (
                SELECT id
                FROM users
                WHERE email = $1
            )
            "#,
        )
        .bind(&email)
        .fetch_one(&db)
        .await
        .expect("password reset token should be queryable");

        assert_eq!(token_hash_count.0, 1);
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

        assert!(matches!(result, Err(AuthServiceError::InvalidToken)));
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

        assert!(matches!(result, Err(AuthServiceError::InvalidToken)));
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

        assert!(matches!(result, Err(AuthServiceError::InvalidPassword)));
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

        assert!(matches!(result, Err(AuthServiceError::InvalidPassword)));
    }

    #[tokio::test]
    async fn complete_registration_rejects_password_shorter_than_8_characters() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "test-token".to_string(),
            "saku".to_string(),
            "1234567".to_string(),
        )
        .await;

        assert!(matches!(result, Err(AuthServiceError::InvalidPassword)));
    }

    #[tokio::test]
    async fn complete_registration_rejects_password_longer_than_128_characters() {
        let db = test_db_pool().await;

        let result = AuthService::complete_registration(
            &db,
            "test-token".to_string(),
            "saku".to_string(),
            "a".repeat(129),
        )
        .await;

        assert!(matches!(result, Err(AuthServiceError::InvalidPassword)));
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

        assert!(matches!(result, Err(AuthServiceError::InvalidUserName)));
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

        assert!(matches!(result, Err(AuthServiceError::InvalidUserName)));
    }

    #[tokio::test]
    async fn complete_registration_creates_user_for_valid_token() {
        let db = test_db_pool().await;

        let token = "valid-registration-token";
        let token_hash = hash_token(token);
        let email = format!("complete-{}@example.com", uuid::Uuid::new_v4());

        AuthRepository::create_pending_registration(&db, &email, &token_hash)
            .await
            .expect("pending registration should be created");

        let result = AuthService::complete_registration(
            &db,
            token.to_string(),
            "saku".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(
            result.is_ok(),
            "complete_registration should succeed for a valid token: {:?}",
            result
        );

        let user = sqlx::query!(
            r#"
            SELECT email, user_name, password_hash
            FROM users
            WHERE email = $1
            "#,
            email
        )
        .fetch_one(&db)
        .await
        .expect("user should be created");

        assert_eq!(user.email, email);
        assert_eq!(user.user_name, "saku");
        assert_ne!(user.password_hash, "password123");
        assert!(!user.password_hash.is_empty());
    }

    #[tokio::test]
    async fn complete_registration_marks_pending_registration_as_completed() {
        let db = test_db_pool().await;

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);
        let email = format!("completed-{}@example.com", uuid::Uuid::new_v4());

        AuthRepository::create_pending_registration(&db, &email, &token_hash)
            .await
            .expect("pending registration should be created");

        AuthService::complete_registration(
            &db,
            token,
            "saku".to_string(),
            "password123".to_string(),
        )
        .await
        .expect("complete_registration should succeed");

        let row = sqlx::query!(
            r#"
            SELECT completed_at
            FROM pending_registrations
            WHERE token_hash = $1
            "#,
            token_hash
        )
        .fetch_one(&db)
        .await
        .expect("pending registration should exist");

        assert!(
            row.completed_at.is_some(), //completed_atがnullでなければok
            "completed_at should be set after complete_registration"
        );
    }

    #[tokio::test]
    async fn complete_registration_rejects_already_completed_token() {
        let db = test_db_pool().await;

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);
        let email = format!("reuse-{}@example.com", uuid::Uuid::new_v4());

        AuthRepository::create_pending_registration(&db, &email, &token_hash)
            .await
            .expect("pending registration should be created");

        AuthService::complete_registration(
            &db,
            token.clone(),
            "saku".to_string(),
            "password123".to_string(),
        )
        .await
        .expect("first complete_registration should succeed");

        let result = AuthService::complete_registration(
            //二回目の本登録。同じトークンなので失敗するはず。
            &db,
            token,
            "another_user".to_string(),
            "password456".to_string(),
        )
        .await;

        assert!(matches!(result, Err(AuthServiceError::InvalidToken)));
    }

    #[tokio::test]
    async fn complete_registration_rejects_expired_token() {
        let db = test_db_pool().await;

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);
        let email = format!("expired-{}@example.com", uuid::Uuid::new_v4());

        sqlx::query!(
            //pending_registrationsにトークン期限、現在時刻-1分で登録。
            r#"
            INSERT INTO pending_registrations (
                email,
                token_hash,
                expires_at
            )
            VALUES (
                $1,
                $2,
                now() - interval '1 minute'
            )
            "#,
            email,
            token_hash
        )
        .execute(&db)
        .await
        .expect("expired pending registration should be created");

        let result = AuthService::complete_registration(
            &db,
            token,
            "saku".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(matches!(result, Err(AuthServiceError::InvalidToken)));
    }

    #[tokio::test]
    async fn complete_registration_rejects_email_already_registered() {
        let db = test_db_pool().await;

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);
        let email = format!("duplicate-{}@example.com", uuid::Uuid::new_v4());

        AuthRepository::create_pending_registration(&db, &email, &token_hash)
            .await
            .expect("pending registration should be created");

        let existing_password_hash =
            hash_password("oldpassword123").expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "existing_user", &existing_password_hash)
            .await
            .expect("existing user should be created");

        let result = AuthService::complete_registration(
            &db,
            token,
            "saku".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(matches!(
            result,
            Err(AuthServiceError::EmailAlreadyRegistered)
        ));

        let user_count = sqlx::query!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM users
            WHERE email = $1
            "#,
            email
        )
        .fetch_one(&db)
        .await
        .expect("user count should be fetched");

        assert_eq!(user_count.count, 1, "duplicate user should not be created");

        let pending_registration = sqlx::query!(
            r#"
            SELECT completed_at
            FROM pending_registrations
            WHERE token_hash = $1
            "#,
            token_hash
        )
        .fetch_one(&db)
        .await
        .expect("pending registration should exist");

        assert!(
            pending_registration.completed_at.is_none(),
            "pending registration should not be marked as completed when email is already registered"
        );
    }
    #[tokio::test]
    async fn complete_registration_rolls_back_user_creation_when_mark_completed_fails() {
        let db = test_db_pool().await;

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);
        let email = format!("rollback-{}@example.com", uuid::Uuid::new_v4());

        AuthRepository::create_pending_registration(&db, &email, &token_hash)
            .await
            .expect("pending registration should be created");

        sqlx::query(
            r#"
            CREATE OR REPLACE FUNCTION fail_pending_registration_completion()
            RETURNS trigger AS $$
            BEGIN
                RAISE EXCEPTION 'forced pending registration completion failure';
            END;
            $$ LANGUAGE plpgsql;
            "#,
        )
        .execute(&db)
        .await
        .expect("failure trigger function should be created");

        sqlx::query(
            r#"
            CREATE TRIGGER fail_pending_registration_completion_trigger
            BEFORE UPDATE OF completed_at
            ON pending_registrations
            FOR EACH ROW
            WHEN (NEW.completed_at IS NOT NULL)
            EXECUTE FUNCTION fail_pending_registration_completion();
            "#,
        )
        .execute(&db)
        .await
        .expect("failure trigger should be created");

        let result = AuthService::complete_registration(
            &db,
            token,
            "saku".to_string(),
            "password123".to_string(),
        )
        .await;

        sqlx::query(
            r#"
            DROP TRIGGER IF EXISTS fail_pending_registration_completion_trigger
            ON pending_registrations;
            "#,
        )
        .execute(&db)
        .await
        .expect("failure trigger should be dropped");

        sqlx::query(
            r#"
            DROP FUNCTION IF EXISTS fail_pending_registration_completion();
            "#,
        )
        .execute(&db)
        .await
        .expect("failure trigger function should be dropped");

        assert!(
            matches!(result, Err(AuthServiceError::Database(_))),
            "complete_registration should fail when completed_at update fails"
        );

        let user_count = sqlx::query!(
            r#"
            SELECT COUNT(*) AS "count!"
            FROM users
            WHERE email = $1
            "#,
            email
        )
        .fetch_one(&db)
        .await
        .expect("user count should be fetched");

        assert_eq!(
            user_count.count, 0,
            "user should be rolled back when pending registration completion fails"
        );

        let pending_registration = sqlx::query!(
            r#"
            SELECT completed_at
            FROM pending_registrations
            WHERE token_hash = $1
            "#,
            token_hash
        )
        .fetch_one(&db)
        .await
        .expect("pending registration should exist");

        assert!(
            pending_registration.completed_at.is_none(),
            "pending registration should remain incomplete after rollback"
        );
    }

    //Session
    #[tokio::test]
    async fn login_accepts_registered_user_with_correct_password() {
        let db = test_db_pool().await;

        let email = format!("login-{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";
        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
            .await
            .expect("user should be created");

        let result = AuthService::login(&db, email.clone(), password.to_string()).await;

        assert!(
            result.is_ok(),
            "login should succeed with correct password: {:?}",
            result
        );

        let output = result.unwrap();

        assert_eq!(output.email, email);
        assert_eq!(output.user_name, "saku");
    }

    #[tokio::test]
    async fn login_rejects_registered_user_with_wrong_password() {
        let db = test_db_pool().await;

        let email = format!("login-wrong-password-{}@example.com", uuid::Uuid::new_v4());
        let password_hash =
            hash_password("correct-password").expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
            .await
            .expect("user should be created");

        let result = AuthService::login(&db, email, "wrong-password".to_string()).await;

        assert!(matches!(result, Err(AuthServiceError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn login_rejects_unknown_email() {
        let db = test_db_pool().await;

        let result = AuthService::login(
            &db,
            "unknown@example.com".to_string(),
            "password123".to_string(),
        )
        .await;

        assert!(matches!(result, Err(AuthServiceError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn login_creates_session_for_registered_user_with_correct_password() {
        let db = test_db_pool().await;

        let email = format!("login-session-{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";
        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(
            //repositoryで直接データベース操作でユーザー作成
            &db,
            &email,
            "saku",
            &password_hash,
        )
        .await
        .expect("user should be created");

        let result = AuthService::login(
            //作ったユーザーでログイン処理
            &db,
            email.clone(),
            password.to_string(),
        )
        .await;

        assert!(
            result.is_ok(),
            "login should succeed with correct password: {:?}",
            result
        );

        let output = result.unwrap();

        assert_eq!(output.email, email);
        assert_eq!(output.user_name, "saku");
        assert!(
            !output.session_token.is_empty(), //セッショントークンが空だったら中止
            "login output should include a raw session token for Set-Cookie"
        );

        let session_token_hash = hash_token(&output.session_token);

        let session = sqlx::query!(
            //メールからセッション,ユーザー検索
            r#"
            SELECT s.session_token_hash, s.expires_at, s.revoked_at
            FROM sessions s
            INNER JOIN users u ON u.id = s.user_id
            WHERE u.email = $1
            "#,
            email
        )
        .fetch_one(&db)
        .await
        .expect("session should be created");

        assert_eq!(session.session_token_hash, session_token_hash);
        assert!(
            session.expires_at > chrono::Utc::now(),
            "session expiry should be in the future"
        );
        assert!(
            session.revoked_at.is_none(),
            "new session should not be revoked"
        );
    }

    #[tokio::test]
    async fn logout_revokes_existing_session() {
        let db = test_db_pool().await;

        let email = format!("logout-{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";
        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
            .await
            .expect("user should be created");

        let login_output = AuthService::login(&db, email, password.to_string())
            .await
            .expect("login should succeed");

        AuthService::logout(&db, login_output.session_token.clone())
            .await
            .expect("logout should succeed");

        let session_token_hash = hash_token(&login_output.session_token);

        let session = sqlx::query!(
            r#"
            SELECT revoked_at
            FROM sessions
            WHERE session_token_hash = $1
            "#,
            session_token_hash
        )
        .fetch_one(&db)
        .await
        .expect("session should exist");

        assert!(session.revoked_at.is_some(), "logout should set revoked_at");
    }

    #[tokio::test]
    async fn current_user_returns_user_for_valid_session() {
        let db = test_db_pool().await;

        let email = format!("me-{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";
        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
            .await
            .expect("user should be created");

        let user = sqlx::query!(
            r#"
            SELECT id
            FROM users
            WHERE email = $1
            "#,
            email
        )
        .fetch_one(&db)
        .await
        .expect("user should exist");

        let login_output = AuthService::login(&db, email.clone(), password.to_string())
            .await
            .expect("login should succeed");

        let result = AuthService::current_user(&db, login_output.session_token).await;

        assert!(
            result.is_ok(),
            "current_user should succeed with valid session: {:?}",
            result
        );

        let output = result.unwrap();

        assert_eq!(output.user_id, user.id);
        assert_eq!(output.email, email);
        assert_eq!(output.user_name, "saku");
        assert_eq!(output.role, "editor");
    }
}
