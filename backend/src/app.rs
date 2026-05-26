use axum::{
    routing::{get, post}, 
    Router
};

use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;


use crate::{
    features::{
        auth::handler::{
            pre_register,
            complete_registration,
            login,
            logout,
            me,
        },
        occurrences::handler::{
            create_occurrence,
            get_occurrence,
        },
    },
    state::AppState,
    openapi::ApiDoc
};


pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        
        .route("/health", get(health))
        .route("/info", get(info))
        //auth
        .route("/auth/pre_register", post(pre_register))
        .route("/auth/complete_registration", post(complete_registration))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        //occurrence
        .route("/occurrences", post(create_occurrence))
        .route("/occurrences/{occurrence_id}", get(get_occurrence))
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
    use crate::config::{AppConfig, Config, PosgreConfig, SmtpConfig, FusekiConfig};
    use crate::state::AppState;
    use crate::features::auth::repository::AuthRepository;
    use crate::features::auth::service::{
        hash_password,
        hash_token,
        AuthService
    };
    use crate::infrastructure::fuseki::FusekiClient;

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode,Method,header},
    };
    use axum::http::header::{CONTENT_TYPE, SET_COOKIE, COOKIE};
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use tower::util::ServiceExt; // oneshot
    use sha2::Digest;

    use crate::features::occurrences::service::{
        OccurrenceRdfStore,
        OccurrenceServiceError,
    };
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    use oxrdfio::{RdfFormat, RdfParser};

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

            fuseki: FusekiConfig{
                base_url: std::env::var("FUSEKI_BASE_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
                user: std::env::var("FUSEKI_USER")
                    .unwrap_or_else(|_| "occurrence_backend".to_string()),
                password: std::env::var("FUSEKI_PASSWORD")
                    .unwrap_or_else(|_| "change_me_backend_password".to_string())
            }
        };

        let posgre = PgPoolOptions::new()
        .connect_lazy(&config.posgre.url)
        .expect("failed to create lazy database pool");

        AppState::new(config,posgre,Arc::new(NoopOccurrenceRdfStore),)
    }

    #[derive(Clone, Default)]
    struct NoopOccurrenceRdfStore;

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for NoopOccurrenceRdfStore {
        async fn save_nquads(
            &self,
            _nquads: Vec<u8>,
        ) -> Result<(), OccurrenceServiceError> {
            Ok(())
        }

        async fn get_occurrence_nquads(
            &self,
            _occurrence_uri: &str,
        ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
            Ok(None)
        }
    }

    fn test_state_with_occurrence_rdf_store(
        occurrence_rdf_store: Arc<dyn OccurrenceRdfStore>,
    ) -> AppState {
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
            smtp: SmtpConfig {
                host: "127.0.0.1".to_string(),
                port: 1025,
                username: "".to_string(),
                password: "".to_string(),
                tls: "none".to_string(),
                from: "no-replay@example.com".to_string(),
            },
            fuseki: FusekiConfig {
                base_url: std::env::var("FUSEKI_BASE_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
                user: std::env::var("FUSEKI_USER")
                    .unwrap_or_else(|_| "occurrence_backend".to_string()),
                password: std::env::var("FUSEKI_PASSWORD")
                    .unwrap_or_else(|_| "change_me_backend_password".to_string()),
            },
        };

        let posgre = PgPoolOptions::new()
            .connect_lazy(&config.posgre.url)
            .expect("failed to create lazy database pool");

        AppState::new(
            config,
            posgre,
            occurrence_rdf_store,
        )
    }

    #[derive(Clone, Default)]
    struct FakeOccurrenceRdfStore {
    saved_nquads: Arc<Mutex<Vec<Vec<u8>>>>,
    occurrence_nquads_by_uri: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl FakeOccurrenceRdfStore {
        fn insert_occurrence_nquads(
            &self,
            occurrence_uri: impl Into<String>,
            nquads: impl Into<Vec<u8>>,
        ) {
            self.occurrence_nquads_by_uri
                .lock()
                .expect("mutex should not be poisoned")
                .insert(occurrence_uri.into(), nquads.into());
        }
    }

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for FakeOccurrenceRdfStore {
        async fn save_nquads(
            &self,
            nquads: Vec<u8>,
        ) -> Result<(), OccurrenceServiceError> {
            self.saved_nquads
                .lock()
                .expect("mutex should not be poisoned")
                .push(nquads);

            Ok(())
        }

        async fn get_occurrence_nquads(
            &self,
            occurrence_uri: &str,
        ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
            Ok(self
                .occurrence_nquads_by_uri
                .lock()
                .expect("mutex should not be poisoned")
                .get(occurrence_uri)
                .cloned())
        }
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

    #[derive(Clone, Default)]
    struct FailingOccurrenceRdfStore {
        attempted_nquads: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for FailingOccurrenceRdfStore {
        async fn save_nquads(
            &self,
            nquads: Vec<u8>,
        ) -> Result<(), OccurrenceServiceError> {
            self.attempted_nquads
                .lock()
                .expect("mutex should not be poisoned")
                .push(nquads);

            Err(OccurrenceServiceError::StoreFailed)
        }

        async fn get_occurrence_nquads(
            &self,
            _occurrence_uri: &str,
        ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
            Err(OccurrenceServiceError::StoreFailed)
        }
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

    #[tokio::test]
    async fn logout_route_returns_unauthorized_without_session_cookie() {
        let state = test_state();
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/logout")
                    .body(Body::empty())
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

        assert_eq!(body["error"], "invalid_session");
        assert_eq!(body["message"], "Invalid session");
    }

    #[tokio::test]
    async fn logout_route_revokes_session_and_clears_cookie() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-logout-{}@example.com", uuid::Uuid::new_v4());
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

        let login_output = AuthService::login(
            &db,
            email,
            password.to_string(),
        )
        .await
        .expect("login should succeed");

        let session_token = login_output.session_token.clone();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/logout")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let set_cookie = response
            .headers()
            .get(SET_COOKIE)
            .expect("logout response should include Set-Cookie header")
            .to_str()
            .expect("Set-Cookie header should be valid string");

        assert!(
            set_cookie.contains("session="),
            "logout response should clear session cookie"
        );

        assert!(
            set_cookie.contains("Max-Age=0"),
            "logout cookie should have Max-Age=0"
        );

        assert!(
            set_cookie.contains("Path=/"),
            "logout cookie should target the same path"
        );

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

        assert!(
            session.revoked_at.is_some(),
            "logout should mark session as revoked"
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body: serde_json::Value = serde_json::from_slice(&body)
            .expect("response body should be JSON");

        assert_eq!(body["message"], "logout successful");
    }

    #[tokio::test]
    async fn me_route_returns_unauthorized_without_session_cookie() {
        let state = test_state();
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/auth/me")
                    .body(Body::empty())
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

        assert_eq!(body["error"], "invalid_session");
        assert_eq!(body["message"], "Invalid session");
    }

    #[tokio::test]
    async fn me_route_returns_current_user_for_valid_session_cookie() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-me-{}@example.com", uuid::Uuid::new_v4());
        let password = "password123";

        let password_hash = hash_password(password)
            .expect("password hash should be created");

        AuthRepository::create_user(
            &db,
            &email,
            "saku",
            &password_hash,
        ) .await .expect("user should be created");
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

        let login_output = AuthService::login(
            &db,
            email.clone(),
            password.to_string(),
        )
        .await
        .expect("login should succeed");

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/auth/me")
                    .header(COOKIE, format!("session={}", login_output.session_token))
                    .body(Body::empty())
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

        assert_eq!(body["user_id"], user.id.to_string());
        assert_eq!(body["email"], email);
        assert_eq!(body["user_name"], "saku");
        assert_eq!(body["role"], "editor");
    }

    #[tokio::test]
    async fn me_route_returns_unauthorized_for_revoked_session_cookie() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-me-revoked-{}@example.com", uuid::Uuid::new_v4());
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

        let login_output = AuthService::login(
            &db,
            email,
            password.to_string(),
        )
        .await
        .expect("login should succeed");

        AuthService::logout(
            &db,
            login_output.session_token.clone(),
        )
        .await
        .expect("logout should succeed");

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/auth/me")
                    .header(COOKIE, format!("session={}", login_output.session_token))
                    .body(Body::empty())
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

        assert_eq!(body["error"], "invalid_session");
        assert_eq!(body["message"], "Invalid session");
    }

    #[tokio::test]
    async fn me_route_returns_unauthorized_for_expired_session_cookie() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-me-expired-{}@example.com", uuid::Uuid::new_v4());
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

        let login_output = AuthService::login(
            &db,
            email,
            password.to_string(),
        )
        .await
        .expect("login should succeed");

        let session_token_hash = hash_token(&login_output.session_token);

        sqlx::query!(
            r#"
            UPDATE sessions
            SET expires_at = now() - interval '1 minute'
            WHERE session_token_hash = $1
            "#,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be expired");

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/auth/me")
                    .header(COOKIE, format!("session={}", login_output.session_token))
                    .body(Body::empty())
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

        assert_eq!(body["error"], "invalid_session");
        assert_eq!(body["message"], "Invalid session");
    }

    //occurrence
    #[tokio::test]
    async fn create_occurrence_route_requires_login() {
        let app = build_app(test_state());

        let nquads = r#"
        _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
        "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .body(Body::from(nquads))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body["error"], "invalid_session");
        assert_eq!(body["message"], "Invalid session");
    }

    #[tokio::test]
    async fn create_occurrence_route_returns_unauthorized_for_invalid_session_cookie() {
        let state = test_state();
        let app = build_app(state);

        let nquads = r#"
        _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
        "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, "session=invalid-session-token")
                    .body(Body::from(nquads))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body["error"], "invalid_session");
        assert_eq!(body["message"], "Invalid session");
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_empty_body() {
        let state = test_state();
        let db = state.posgre.clone();

        let email = format!("occurrence-user-{}@example.com", uuid::Uuid::new_v4());
        let user_name = "occurrence-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from("")) //テスト用に空body
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX) //こっちはレスポンスの変換 bodyはレスポンスのbody
            .await
            .unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body["error"], "empty_body");
        assert_eq!(body["message"], "Request body must not be empty");
    }

    #[tokio::test]
    async fn create_occurrence_route_with_valid_session_returns_created() {
        let state = test_state();
        let db = state.posgre.clone();

        let email = format!("occurrence-user-{}@example.com", uuid::Uuid::new_v4());
        let user_name = "occurrence-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let nquads = r#"
    _:occurrence <https://example.org/vocab/taxonName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(nquads))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        let occurrence_id = body["occurrence_id"]
            .as_str()
            .expect("occurrence_id should be string");

        let occurrence_uri = body["occurrence_uri"]
            .as_str()
            .expect("occurrence_uri should be string");

        assert!(
            occurrence_uri.starts_with("https://bio-database.net/occurrences/")
        );

        assert!(
            occurrence_uri.ends_with(occurrence_id),
            "occurrence_uri should contain occurrence_id"
        );

        uuid::Uuid::parse_str(occurrence_id)
            .expect("occurrence_id should be valid UUID");
    }

    #[tokio::test]
    async fn create_occurrence_route_with_valid_session_saves_nquads_to_store() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!("occurrence-store-user-{}@example.com", uuid::Uuid::new_v4());
        let user_name = "occurrence-store-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        let occurrence_uri = body_json["occurrence_uri"]
            .as_str()
            .expect("response should contain occurrence_uri");

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            1,
            "POST /occurrences should save exactly one N-Quads document to OccurrenceRdfStore"
        );

        let saved_nquads = &saved[0];

        let parsed_quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(saved_nquads)
            .collect::<Result<Vec<_>, _>>()
            .expect("saved N-Quads should be valid");

        assert_eq!(
            parsed_quads.len(),
            2,
            "saved N-Quads should contain frontend quad plus backend creator quad"
        );

        let expected_subject = format!("<{}>", occurrence_uri);

        assert!(
            parsed_quads.iter().all(|quad| {
                quad.subject.to_string() == expected_subject
            }),
            "all saved quads should use backend-issued occurrence URI as subject"
        );

        let has_frontend_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<https://example.org/vocab/scientificName>"
                && quad.object.to_string() == "\"Lumbricus terrestris\""
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_frontend_quad,
            "saved N-Quads should contain the frontend occurrence data"
        );

        let expected_creator_object =
            format!("<https://bio-database.net/users/{}>", user_id);

        let has_creator_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string() == expected_creator_object
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "saved N-Quads should contain backend-confirmed creator user URI"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_with_invalid_nquads_returns_bad_request_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-invalid-rdf-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-invalid-rdf-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let invalid_nquads = br#"
    this is not valid n-quads
    "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(invalid_nquads.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "invalid_rdf");
        assert_eq!(body_json["message"], "Invalid RDF body");

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "invalid N-Quads should not be saved to OccurrenceRdfStore"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_when_rdf_store_fails_returns_bad_gateway() {
        let store = FailingOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-rdf-store-fail-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-rdf-store-fail-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "rdf_store_error");
        assert_eq!(body_json["message"], "Failed to save occurrence RDF");

        let attempted = store
            .attempted_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            attempted.len(),
            1,
            "valid occurrence RDF should be passed to the store even if the store fails"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_frontend_creator_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-reject-creator-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-reject-creator-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    _:occurrence <http://purl.org/dc/terms/creator> <https://bio-database.net/users/fake-user> <https://bio-database.net/graphs/occurrences> .
    "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "forbidden_rdf_predicate");
        assert_eq!(
            body_json["message"],
            "Frontend RDF must not contain backend-managed predicates"
        );

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "RDF containing frontend-supplied creator should not be saved"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_non_occurrence_graph_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-wrong-graph-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-wrong-graph-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let frontend_nquads = br#"
    _:occurrence <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/taxonomy> .
    "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "forbidden_rdf_graph");
        assert_eq!(
            body_json["message"],
            "Occurrence RDF must use the occurrence graph"
        );

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "RDF using a non-occurrence named graph should not be saved"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_empty_rdf_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-empty-rdf-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-empty-rdf-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let empty_rdf_body = br#"
        
        
    "#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(empty_rdf_body.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "empty_rdf");
        assert_eq!(
            body_json["message"],
            "Occurrence RDF must contain at least one quad"
        );

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "empty RDF should not be saved"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn create_occurrence_route_saves_data_to_real_fuseki() {
        dotenvy::dotenv().ok();

        let database_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for integration test");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            smtp: SmtpConfig {
                host: "127.0.0.1".to_string(),
                port: 1025,
                username: "".to_string(),
                password: "".to_string(),
                tls: "none".to_string(),
                from: "no-replay@example.com".to_string(),
            },
            fuseki: FusekiConfig {
                base_url: std::env::var("FUSEKI_BASE_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:3033/occurrence".to_string()),
                user: std::env::var("FUSEKI_USER")
                    .unwrap_or_else(|_| "occurrence_backend".to_string()),
                password: std::env::var("FUSEKI_PASSWORD")
                    .unwrap_or_else(|_| "change_me_backend_password".to_string()),
            },
        };

        let posgre = PgPoolOptions::new()
            .connect_lazy(&config.posgre.url)
            .expect("failed to create lazy database pool");

        let fuseki_client = FusekiClient::new(config.fuseki.clone());

        let state = AppState::new(
            config.clone(),
            posgre,
            Arc::new(fuseki_client),
        );

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-real-fuseki-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-real-fuseki-user";
        let password_hash = hash_password("password123")
            .expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            user_name,
            password_hash
        )
        .fetch_one(&db)
        .await
        .expect("user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let app = build_app(state);

        let scientific_name = format!(
            "Lumbricus terrestris {}",
            uuid::Uuid::new_v4()
        );

        let frontend_nquads = format!(
            r#"
    _:occurrence <https://example.org/vocab/scientificName> "{}" <https://bio-database.net/graphs/occurrences> .
    "#,
            scientific_name
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.into_bytes()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        let occurrence_uri = body_json["occurrence_uri"]
            .as_str()
            .expect("response should contain occurrence_uri");

        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "https://example.org/vocab/scientificName";
        let creator_predicate = "http://purl.org/dc/terms/creator";
        let expected_user_uri = format!("https://bio-database.net/users/{}", user_id);

        let ask_query = format!(
            r#"
            ASK WHERE {{
                GRAPH <{}> {{
                <{}> <{}> "{}" .
                <{}> <{}> <{}> .
                }}
            }}
            "#,
            graph_uri,
            occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            occurrence_uri,
            creator_predicate,
            expected_user_uri
        );

        let sparql_url = format!(
            "{}/sparql",
            config.fuseki.base_url.trim_end_matches('/')
        );

        let ask_response = reqwest::Client::new()
            .post(sparql_url)
            .basic_auth(&config.fuseki.user, Some(&config.fuseki.password))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/sparql-query",
            )
            .header(
                reqwest::header::ACCEPT,
                "application/sparql-results+json",
            )
            .body(ask_query)
            .send()
            .await
            .expect("SPARQL ASK request should be sent");

        assert!(
            ask_response.status().is_success(),
            "SPARQL ASK should succeed, got {}",
            ask_response.status()
        );

        let ask_body: serde_json::Value = ask_response
            .json()
            .await
            .expect("SPARQL ASK response should be JSON");

        assert_eq!(
            ask_body["boolean"], true,
            "POST /occurrences should save occurrence data and creator to real Fuseki"
        );
    }

    #[tokio::test]
    async fn get_occurrence_route_returns_nquads_for_existing_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            occurrence_id
        );

        let expected_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/test-user> <https://bio-database.net/graphs/occurrences> .
    "#,
            occurrence_uri,
            occurrence_uri,
        );

        store.insert_occurrence_nquads(
            occurrence_uri.clone(),
            expected_nquads.clone().into_bytes(),
        );

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .expect("response should have Content-Type")
            .to_str()
            .expect("Content-Type should be valid string");

        assert!(
            content_type.starts_with("application/n-quads"),
            "GET /occurrences/{{occurrence_id}} should return application/n-quads"
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        assert_eq!(
            body.as_ref(),
            expected_nquads.as_bytes(),
            "response body should be occurrence N-Quads returned from OccurrenceRdfStore"
        );
    }

    #[tokio::test]
    async fn get_occurrence_route_returns_not_found_for_missing_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let app = build_app(state);

        let occurrence_id = uuid::Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "occurrence_not_found");
        assert_eq!(body_json["message"], "Occurrence not found");
    }

    #[tokio::test]
    async fn get_occurrence_route_when_rdf_store_fails_returns_bad_gateway() {
        let store = FailingOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(
            Arc::new(store.clone()),
        );

        let app = build_app(state);

        let occurrence_id = uuid::Uuid::new_v4();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "rdf_store_error");
        assert_eq!(body_json["message"], "Failed to save occurrence RDF");
    }
}