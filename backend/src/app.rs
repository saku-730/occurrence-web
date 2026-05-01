use axum::{
    routing::{get, post}, 
    Router
};

use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;


use crate::{
    features::auth::handler::pre_register,
    state::AppState,
    openapi::ApiDoc
};


pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/auth/pre_register", post(pre_register))
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
    use crate::config::{AppConfig, Config, PosgreConfig};
    use crate::state::AppState;

    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode,Method,header},
    };
    use sqlx::{postgres::PgPoolOptions, PgPool};
    use tower::util::ServiceExt; // oneshot

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
}