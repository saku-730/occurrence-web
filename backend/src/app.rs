use axum::{
    routing::{get, post}, 
    Router
};

use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;


use crate::{
    features::auth::handler::{
        pre_register,
        complete_registration,
        login,
    },
    state::AppState,
    openapi::ApiDoc
};


pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/auth/pre_register", post(pre_register))
        .route("/auth/complete_registration", post(complete_registration))
        .route("/auth/login", post(login))
        .merge(
            SwaggerUi::new("/swagger-ui")
                .url("/openapi.json", ApiDoc::openapi()),
        )

        .with_state(state)
}

async fn index() -> &'static str {
    "Occurrence App Backend"
}

async fn health() -> &'static str {
    "ok"
}

use axum::extract::State;

async fn info(State(state): State<AppState>) -> String {
    state.config.app.app_base_url.clone()
}


#[cfg(test)]//test section
mod tests {
    use super::build_app;
    use crate::config::{AppConfig, Config, PosgreConfig, SmtpConfig};
    use crate::state::AppState;
    use crate::features::auth::repository::AuthRepository;
    use crate::features::auth::service::hash_password;

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode,Method,header},
    };
    use axum::http::header::{CONTENT_TYPE, SET_COOKIE};
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use tower::util::ServiceExt; // oneshot
    use sha2::Digest;

    fn test_state() -> AppState {
        dotenvy::dotenv().ok();

        let database_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for app tests");
            
        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },

            smtp: SmtpConfig{
                host: "127.0.0.1".to_string(),
                port: 1025,
                username: "".to_string(),
                password: "".to_string(),
                tls: "none".to_string(),
                from: "no-replay@example.com".to_string(),
            },
        };

        let posgre = PgPoolOptions::new()
        .connect_lazy(&config.posgre.url)
        .expect("failed to create lazy database pool");

        AppState::new(config,posgre)
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

    async fn delete_mailpit_messages() {
        let response = reqwest::Client::new()
            .delete("http://127.0.0.1:8025/api/v1/messages")
            .send()
            .await
            .expect("failed to delete Mailpit messages");

        assert!(
            response.status().is_success(),
            "failed to clear Mailpit messages: {}",
            response.status()
        );
    }

    async fn fetch_mailpit_messages() -> Vec<serde_json::Value> {
        let response = reqwest::get("http://127.0.0.1:8025/api/v1/messages")
            .await
            .expect("failed to fetch Mailpit messages");

        assert!(
            response.status().is_success(),
            "failed to fetch Mailpit messages: {}",
            response.status()
        );

        let body: serde_json::Value = response
            .json()
            .await
            .expect("failed to parse Mailpit messages response");

        body["messages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    async fn fetch_mailpit_message(message_id: &str) -> serde_json::Value {
        let url = format!("http://127.0.0.1:8025/api/v1/message/{}", message_id);

        let response = reqwest::get(url)
            .await
            .expect("failed to fetch Mailpit message detail");

        assert!(
            response.status().is_success(),
            "failed to fetch Mailpit message detail: {}",
            response.status()
        );

        response
            .json()
            .await
            .expect("failed to parse Mailpit message detail response")
    }

    #[tokio::test]
    async fn index_route_returns_backend_name() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"Occurrence App Backend");
    }

    #[tokio::test]
    async fn health_route_returns_ok() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn register_route_returns() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/pre_register")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"email":"test@example.com"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], br#"{"message":"temporary registration accepted","email":"test@example.com"}"#);
    }

    #[tokio::test]
    async fn register_route_rejects_missing_json_body() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/pre_register")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.status().is_client_error());
    }
    
    #[tokio::test]
    async fn register_route_returns_created_json_for_valid_email() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/pre_register")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"email":"test@example.com"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            &body[..],
            br#"{"message":"temporary registration accepted","email":"test@example.com"}"#
        );
    }

    #[tokio::test]
    async fn register_route_returns_bad_request_for_invalid_email() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/pre_register")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"email":"invalid-email"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_email");
        assert_eq!(json["message"], "Invalid email");
    }

    #[tokio::test]
    async fn openapi_json_returns_auth_register_spec() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json["openapi"].is_string());
        assert!(json["paths"]["/auth/pre_register"]["post"].is_object());
        assert!(json["components"]["schemas"]["RegisterRequest"].is_object());
        assert!(json["components"]["schemas"]["RegisterResponse"].is_object());
        assert!(json["components"]["schemas"]["ErrorResponse"].is_object());
    }
    
    #[tokio::test]
    async fn openapi_json_includes_complete_registration_response_statuses() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body = String::from_utf8(body.to_vec()).unwrap();

        assert!(
            body.contains("/auth/complete_registration"),
            "OpenAPI JSON should contain /auth/complete_registration"
        );

        assert!(
            body.contains("\"201\""),
            "OpenAPI JSON should contain 201 response"
        );

        assert!(
            body.contains("\"400\""),
            "OpenAPI JSON should contain 400 response"
        );

        assert!(
            body.contains("\"409\""),
            "OpenAPI JSON should contain 409 response"
        );

        assert!(
            body.contains("\"500\""),
            "OpenAPI JSON should contain 500 response"
        );

        assert!(
            body.contains("CompleteRegistrationRequest"),
            "OpenAPI JSON should contain CompleteRegistrationRequest schema"
        );

        assert!(
            body.contains("CompleteRegistrationResponse"),
            "OpenAPI JSON should contain CompleteRegistrationResponse schema"
        );
    }
        
    #[tokio::test]
    async fn pre_register_route_creates_pending_registration() {
        let state = test_state();
        let db = state.posgre.clone();

        let email = format!("route-valid-{}@example.com", uuid::Uuid::new_v4());

        delete_pending_registration_by_email(&db, &email).await;

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/pre_register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"email":"{}"}}"#, email)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let count = count_pending_registration_by_email(&db, &email).await;
        assert_eq!(count, 1);

        delete_pending_registration_by_email(&db, &email).await;
    }

    
    

    #[tokio::test]
    async fn pre_register_route_rejects_invalid_email_and_does_not_create_pending_registration() {
        let state = test_state();
        let db = state.posgre.clone();

        let invalid_email = format!("invalid-{}", uuid::Uuid::new_v4());

        delete_pending_registration_by_email(&db, &invalid_email).await;

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/pre_register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"email":"{}"}}"#, invalid_email)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["error"], "invalid_email");
        assert_eq!(body_json["message"], "Invalid email");

        let count = count_pending_registration_by_email(&db, &invalid_email).await;
        assert_eq!(count, 0);

        delete_pending_registration_by_email(&db, &invalid_email).await;
    }

    #[tokio::test]
    async fn openapi_json_includes_pre_register_response_statuses() {
        let app = build_app(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let responses = &body_json["paths"]["/auth/pre_register"]["post"]["responses"];

        assert!(responses.get("201").is_some());
        assert!(responses.get("400").is_some());
        assert!(responses.get("500").is_some());
    }

    #[tokio::test]
    async fn pre_register_route_sends_registration_completion_email() {
        let state = test_state();
        let db = state.posgre.clone();

        let email = format!("route-mail-{}@example.com", uuid::Uuid::new_v4());

        delete_pending_registration_by_email(&db, &email).await;
        delete_mailpit_messages().await;

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/pre_register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"email":"{}"}}"#, email)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let mailpit_messages = fetch_mailpit_messages().await; //メール一覧取得

        let message = mailpit_messages
            .iter()
            .find(|message| message["To"] //宛先で特定のメール探索
                .as_array()
                .is_some_and(|to| {
                    to.iter().any(|recipient| {
                        recipient["Address"]
                            .as_str()
                            .is_some_and(|address| address == email)
                    })
                }))
            .expect("registration completion email was not sent");

        let subject = message["Subject"].as_str().unwrap_or("");
        assert!(subject.contains("registration"));

        let message_id = message["ID"]
            .as_str()
            .expect("mailpit message ID is missing");

        let message_detail = fetch_mailpit_message(message_id).await;

        let body = message_detail["Text"].as_str().unwrap_or("");

        assert!(body.contains("/auth/complete_registration"));
        assert!(body.contains("token="));

        delete_pending_registration_by_email(&db, &email).await;
        delete_mailpit_messages().await;
    }

    #[tokio::test]
    async fn complete_registration_route_rejects_missing_json_body() {
        let state = test_state();
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/complete_registration")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(
            response.status().is_client_error(),
            "missing JSON body should return client error"
        );
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn complete_registration_route_creates_user_for_valid_token() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hex::encode(sha2::Sha256::digest(token.as_bytes()));
        let email = format!("route-complete-{}@example.com", uuid::Uuid::new_v4());

        AuthRepository::create_pending_registration(
            &db,
            &email,
            &token_hash,
        )
        .await
        .expect("pending registration should be created");

        let body = serde_json::json!({
            "token": token,
            "user_name": "saku",
            "password": "password123"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/complete_registration")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

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
    async fn complete_registration_route_returns_conflict_for_email_already_registered() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hex::encode(sha2::Sha256::digest(token.as_bytes()));
        let email = format!("route-duplicate-{}@example.com", uuid::Uuid::new_v4());

        AuthRepository::create_pending_registration(
            &db,
            &email,
            &token_hash,
        )
        .await
        .expect("pending registration should be created");

        AuthRepository::create_user(
            &db,
            &email,
            "existing_user", "$argon2id$dummy-existing-password-hash",)
        .await
        .expect("existing user should be created");

        let body = serde_json::json!({
            "token": token,
            "user_name": "saku",
            "password": "password123"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/complete_registration")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body: serde_json::Value = serde_json::from_slice(&body)
            .expect("response body should be JSON");

        assert_eq!(body["error"], "email_already_registered");
        assert_eq!(body["message"], "Email already registered");
    }

    // Session
    #[tokio::test]
    async fn login_route_rejects_missing_json_body() {
        let state = test_state();
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(
            response.status().is_client_error(),
            "missing JSON body should return client error"
        );

        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "/auth/login route should exist"
        );
    }

    #[tokio::test]
    async fn login_route_returns_ok_for_registered_user_with_correct_password() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-login-{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        let password_hash = hash_password(password)
            .expect("password hash should be created");

        AuthRepository::create_user(
            &db,
            &email,
            "saku",
            &password_hash,
        )
        .await
        .expect("user should be created");

        let body = serde_json::json!({
            "email": email,
            "password": password
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body: serde_json::Value = serde_json::from_slice(&body)
            .expect("response body should be JSON");

        assert_eq!(body["message"], "login successful");
        assert_eq!(body["email"], email);
        assert_eq!(body["user_name"], "saku");
    }

    #[tokio::test]
    async fn login_route_returns_unauthorized_for_unknown_email() {
        let state = test_state();
        let app = build_app(state);

        let body = serde_json::json!({
            "email": "unknown@example.com",
            "password": "password123"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body: serde_json::Value = serde_json::from_slice(&body)
            .expect("response body should be JSON");

        assert_eq!(body["error"], "invalid_credentials");
        assert_eq!(body["message"], "Invalid credential");
    }
    
    #[tokio::test]
    async fn login_route_sets_session_cookie_for_registered_user() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-login-cookie-{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        let password_hash = hash_password(password)
            .expect("password hash should be created");

        AuthRepository::create_user(
            &db,
            &email,
            "saku",
            &password_hash,
        )
        .await
        .expect("user should be created");

        let body = serde_json::json!({
            "email": email,
            "password": password
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/login")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let set_cookie = response
            .headers()
            .get(SET_COOKIE)
            .expect("login response should include Set-Cookie header")
            .to_str()
            .expect("Set-Cookie header should be valid string");

        assert!(
            set_cookie.contains("session="),
            "Set-Cookie should contain session token"
        );

        assert!(
            set_cookie.contains("HttpOnly"),
            "session cookie should be HttpOnly"
        );

        assert!(
            set_cookie.contains("SameSite=Lax"),
            "session cookie should set SameSite=Lax"
        );

        assert!(
            set_cookie.contains("Path=/"),
            "session cookie should be available for the whole app"
        );
    }
}