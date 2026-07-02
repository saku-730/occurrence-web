use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};

use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    features::{
        auth::handler::{
            complete_registration, login, logout, me, pre_register, request_password_reset,
            reset_password,
        },
        media::handler::{MEDIA_REQUEST_BODY_LIMIT_BYTES, delete_media, get_media, upload_media},
        occurrences::handler::{
            create_occurrence, delete_occurrence, get_occurrence, search_occurrences,
            update_occurrence,
        },
    },
    openapi::ApiDoc,
    state::AppState,
};

// route定義はここに集約する。OpenAPIとhandler追加漏れを見つけやすくするため。
pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/info", get(info))
        // auth: ユーザー登録、ログイン、セッション確認を扱う。
        .route("/auth/pre_register", post(pre_register))
        .route("/auth/complete_registration", post(complete_registration))
        .route("/auth/request_password_reset", post(request_password_reset))
        .route("/auth/reset_password", post(reset_password))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        // media: 添付ファイルのupload/download/deleteを扱う。まずはuploadを接続する。
        .route(
            "/media",
            post(upload_media).layer(DefaultBodyLimit::max(MEDIA_REQUEST_BODY_LIMIT_BYTES)),
        )
        .route("/media/{media_id}", get(get_media).delete(delete_media))
        // occurrence: RDF本体の作成・検索・詳細・更新・削除を扱う。
        .route("/occurrences", post(create_occurrence))
        .route("/occurrences/search", post(search_occurrences))
        .route(
            "/occurrences/{occurrence_id}",
            get(get_occurrence)
                .put(update_occurrence)
                .delete(delete_occurrence),
        )
        .merge(SwaggerUi::new("/swagger-ui").url("/openapi.json", ApiDoc::openapi()))
        .with_state(state)
}

async fn index() -> &'static str {
    "Occurrence App Backend"
}

async fn health() -> &'static str {
    "ok"
}

use axum::extract::State;

// 動作中のbase URLを確認する軽量endpoint。設定の読み込み確認にも使う。
async fn info(State(state): State<AppState>) -> String {
    state.config.app.app_base_url.clone()
}

#[cfg(test)] //test section
mod tests {
    use super::build_app;
    use crate::config::{AppConfig, Config, FusekiConfig, GarageConfig, PosgreConfig, SmtpConfig};
    use crate::features::auth::repository::AuthRepository;
    use crate::features::auth::service::{AuthService, hash_password, hash_token};
    use crate::infrastructure::fuseki::FusekiClient;
    use crate::infrastructure::garage::GarageMediaObjectStore;
    use crate::state::AppState;

    use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, COOKIE, SET_COOKIE};
    use axum::{
        body::{Body, Bytes, to_bytes},
        http::{Method, Request, StatusCode, header},
    };
    use sha2::Digest;
    use sqlx::{PgPool, postgres::PgPoolOptions};
    use tower::util::ServiceExt; // oneshot

    use crate::features::media::service::{
        DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectByteStream, MediaObjectStore,
        MediaServiceError, PutMediaObjectInput,
    };
    use crate::features::occurrences::service::{
        OccurrenceRdfStore, OccurrenceServiceError, SearchOccurrenceStoreRow,
        SearchOccurrencesStoreInput, SearchOccurrencesStorePage, SearchVisibility,
    };
    use oxrdfio::{RdfFormat, RdfParser};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::{Arc, Mutex};

    // multipart正常系でinferの実データ判定を通すJPEG signature。
    const TEST_JPEG_BYTES: &[u8] = &[0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0x00];

    // appテストはrouterを直接叩くため、実HTTP serverを立てずにAppStateだけ構築する。
    fn test_state() -> AppState {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for app tests");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },

            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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

        AppState::new(config, posgre, Arc::new(NoopOccurrenceRdfStore))
    }

    #[derive(Clone, Default)]
    struct NoopOccurrenceRdfStore;

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for NoopOccurrenceRdfStore {
        async fn save_nquads(&self, _nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
            Ok(())
        }

        async fn get_occurrence_nquads(
            &self,
            _occurrence_uri: &str,
        ) -> Result<Option<Vec<u8>>, OccurrenceServiceError> {
            Ok(None)
        }
    }

    fn test_state_with_media_object_store(
        media_object_store: Arc<dyn MediaObjectStore>,
    ) -> AppState {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for app tests");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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

        AppState::new_with_media_object_store(
            config,
            posgre,
            Arc::new(NoopOccurrenceRdfStore),
            media_object_store,
        )
    }

    // occurrence系appテストではRDF storeを差し替え、handlerからserviceまでの接続を検証する。
    fn test_state_with_occurrence_rdf_store(
        occurrence_rdf_store: Arc<dyn OccurrenceRdfStore>,
    ) -> AppState {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for app tests");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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

        AppState::new(config, posgre, occurrence_rdf_store)
    }

    #[derive(Clone, Default)]
    struct FakeOccurrenceRdfStore {
        saved_nquads: Arc<Mutex<Vec<Vec<u8>>>>,
        occurrence_nquads_by_uri: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        search_page: Arc<Mutex<Option<SearchOccurrencesStorePage>>>,
        requested_search_inputs: Arc<Mutex<Vec<(u32, Option<String>)>>>,
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

        fn set_search_page(&self, page: SearchOccurrencesStorePage) {
            *self
                .search_page
                .lock()
                .expect("mutex should not be poisoned") = Some(page);
        }

        fn requested_search_inputs(&self) -> Vec<(u32, Option<String>)> {
            self.requested_search_inputs
                .lock()
                .expect("mutex should not be poisoned")
                .clone()
        }
    }

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for FakeOccurrenceRdfStore {
        async fn save_nquads(&self, nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
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

        async fn is_media_referenced_by_public_occurrence(
            &self,
            media_uri: &str,
        ) -> Result<bool, OccurrenceServiceError> {
            let media_object = format!("<{media_uri}>");
            let public_access = "<https://bio-database.net/terms/access-rights/public>";

            Ok(self
                .occurrence_nquads_by_uri
                .lock()
                .expect("mutex should not be poisoned")
                .values()
                .any(|nquads| {
                    let nquads = String::from_utf8_lossy(nquads);
                    nquads.contains(&media_object) && nquads.contains(public_access)
                }))
        }

        async fn replace_occurrence_nquads(
            &self,
            occurrence_uri: &str,
            nquads: Vec<u8>,
        ) -> Result<(), OccurrenceServiceError> {
            self.occurrence_nquads_by_uri
                .lock()
                .expect("mutex should not be poisoned")
                .insert(occurrence_uri.to_string(), nquads);

            Ok(())
        }

        async fn delete_occurrence_nquads(
            &self,
            occurrence_uri: &str,
        ) -> Result<(), OccurrenceServiceError> {
            self.occurrence_nquads_by_uri
                .lock()
                .expect("mutex should not be poisoned")
                .remove(occurrence_uri);

            Ok(())
        }

        async fn search_occurrences(
            &self,
            input: SearchOccurrencesStoreInput,
        ) -> Result<SearchOccurrencesStorePage, OccurrenceServiceError> {
            let SearchOccurrencesStoreInput {
                filters,
                limit,
                cursor,
                visibility,
            } = input;

            self.requested_search_inputs
                .lock()
                .expect("mutex should not be poisoned")
                .push((limit, cursor));

            let mut page = self
                .search_page
                .lock()
                .expect("mutex should not be poisoned")
                .clone()
                .ok_or(OccurrenceServiceError::StoreFailed)?;

            let row_count_before_visibility = page.rows.len();

            page.rows.retain(|row| match &visibility {
                SearchVisibility::PublicOnly => row.access_rights.as_deref() != Some("private"),
                SearchVisibility::PublicOrOwnPrivate { user_id } => {
                    row.access_rights.as_deref() != Some("private")
                        || row.creator_user_id == Some(*user_id)
                }
                SearchVisibility::All => true,
            });

            let visibility_removed_all_rows =
                row_count_before_visibility > 0 && page.rows.is_empty();

            if !filters.is_empty() {
                page.rows.retain(|row| {
                    filters.iter().all(|filter| {
                        filter.predicate == "http://rs.tdwg.org/dwc/terms/scientificName"
                            && filter.value_type == "literal"
                            && filter.match_type == "exact"
                            && row
                                .scientific_name
                                .as_deref()
                                .is_some_and(|scientific_name| {
                                    scientific_name.to_lowercase()
                                        == filter.value.trim().to_lowercase()
                                })
                    })
                });
            }

            if visibility_removed_all_rows || !filters.is_empty() {
                page.has_next = false;
                page.next_cursor = None;
            }

            Ok(page)
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

        body["messages"].as_array().cloned().unwrap_or_default()
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
    struct DeleteFailingOccurrenceRdfStore {
        occurrence_nquads_by_uri: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        attempted_delete_uris: Arc<Mutex<Vec<String>>>,
    }

    impl DeleteFailingOccurrenceRdfStore {
        fn insert_occurrence_nquads(&self, occurrence_uri: String, nquads: Vec<u8>) {
            self.occurrence_nquads_by_uri
                .lock()
                .expect("mutex should not be poisoned")
                .insert(occurrence_uri, nquads);
        }
    }

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for DeleteFailingOccurrenceRdfStore {
        async fn save_nquads(&self, _nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
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

        async fn delete_occurrence_nquads(
            &self,
            occurrence_uri: &str,
        ) -> Result<(), OccurrenceServiceError> {
            self.attempted_delete_uris
                .lock()
                .expect("mutex should not be poisoned")
                .push(occurrence_uri.to_string());

            Err(OccurrenceServiceError::StoreFailed)
        }
    }

    #[derive(Clone, Default)]
    struct FailingOccurrenceRdfStore {
        attempted_nquads: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    #[async_trait::async_trait]
    impl OccurrenceRdfStore for FailingOccurrenceRdfStore {
        async fn save_nquads(&self, nquads: Vec<u8>) -> Result<(), OccurrenceServiceError> {
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
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
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
        assert_eq!(
            &body[..],
            br#"{"message":"temporary registration accepted","email":"test@example.com"}"#
        );
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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
            .find(|message| {
                message["To"] //宛先で特定のメール探索
                    .as_array()
                    .is_some_and(|to| {
                        to.iter().any(|recipient| {
                            recipient["Address"]
                                .as_str()
                                .is_some_and(|address| address == email)
                        })
                    })
            })
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
    async fn request_password_reset_route_sends_reset_mail_for_registered_email() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-password-reset-{}@example.com", uuid::Uuid::new_v4());
        let password_hash = hash_password("password123").expect("password hash should be created");

        // app経由テストではHTTP handlerからservice/repository/mailまで接続されることを確認する。
        // 登録済みユーザーだけがリセット対象なので、先にusersへ対象ユーザーを作成する。
        AuthRepository::create_user(&db, &email, "reset-user", &password_hash)
            .await
            .expect("user should be created");

        delete_mailpit_messages().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/request_password_reset")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"email":"{}"}}"#, email)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

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

        let mailpit_messages = fetch_mailpit_messages().await;

        let message = mailpit_messages
            .iter()
            .find(|message| {
                message["To"].as_array().is_some_and(|to| {
                    to.iter().any(|recipient| {
                        recipient["Address"]
                            .as_str()
                            .is_some_and(|address| address == email)
                    })
                })
            })
            .expect("password reset email was not sent");

        let subject = message["Subject"].as_str().unwrap_or("");
        assert!(subject.contains("パスワードリセット"));

        let message_id = message["ID"]
            .as_str()
            .expect("mailpit message ID is missing");

        let message_detail = fetch_mailpit_message(message_id).await;
        let body = message_detail["Text"].as_str().unwrap_or("");

        assert!(body.contains("/auth/reset_password"));
        assert!(body.contains("token="));

        delete_mailpit_messages().await;
    }

    #[derive(Clone)]
    struct ReadableAppMediaObjectStore {
        expected_bucket: String,
        expected_object_key: String,
        bytes: Arc<Vec<u8>>,
    }

    #[async_trait::async_trait]
    impl MediaObjectStore for ReadableAppMediaObjectStore {
        async fn put_object(&self, _input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
            Ok(())
        }

        async fn get_object(
            &self,
            input: GetMediaObjectInput,
        ) -> Result<MediaObjectByteStream, MediaServiceError> {
            assert_eq!(input.bucket, self.expected_bucket);
            assert_eq!(input.object_key, self.expected_object_key);
            let bytes = self.bytes.clone();
            Ok(Box::pin(futures_util::stream::once(async move {
                Ok(Bytes::copy_from_slice(bytes.as_slice()))
            })))
        }

        async fn delete_object(
            &self,
            _input: DeleteMediaObjectInput,
        ) -> Result<(), MediaServiceError> {
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct FailingMediaObjectStore {
        attempted_paths: Arc<Mutex<Vec<PathBuf>>>,
    }

    #[async_trait::async_trait]
    impl MediaObjectStore for FailingMediaObjectStore {
        async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
            self.attempted_paths
                .lock()
                .expect("failed media store paths lock should not be poisoned")
                .push(input.file_path);
            Err(MediaServiceError::ObjectStoreFailed)
        }

        async fn get_object(
            &self,
            _input: GetMediaObjectInput,
        ) -> Result<MediaObjectByteStream, MediaServiceError> {
            Err(MediaServiceError::ObjectStoreFailed)
        }

        async fn delete_object(
            &self,
            _input: DeleteMediaObjectInput,
        ) -> Result<(), MediaServiceError> {
            Ok(())
        }
    }

    async fn create_media_test_session(db: &PgPool, prefix: &str) -> (uuid::Uuid, String) {
        let email = format!("{prefix}-{}@example.com", uuid::Uuid::new_v4());
        let password_hash = hash_password("password123").expect("password should be hashed");
        let user_id = sqlx::query_scalar::<_, uuid::Uuid>(
            "INSERT INTO users (email, user_name, password_hash) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(email)
        .bind(prefix)
        .bind(password_hash)
        .fetch_one(db)
        .await
        .expect("media test user should be inserted");
        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);
        sqlx::query(
            "INSERT INTO sessions (user_id, session_token_hash, expires_at) VALUES ($1, $2, now() + interval \x2730 days\x27)",
        )
        .bind(user_id)
        .bind(session_token_hash)
        .execute(db)
        .await
        .expect("media test session should be inserted");

        (user_id, session_token)
    }

    fn multipart_file_body(
        boundary: &str,
        filename: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Vec<u8> {
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        body
    }

    #[derive(Clone, Default)]
    struct RecordingMediaObjectStore {
        written_objects: Arc<Mutex<Vec<RecordedMediaObjectWrite>>>,
        deleted_objects: Arc<Mutex<Vec<DeleteMediaObjectInput>>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RecordedMediaObjectWrite {
        bucket: String,
        object_key: String,
        content_type: String,
        source_path: PathBuf,
        bytes: Vec<u8>,
    }

    #[async_trait::async_trait]
    impl MediaObjectStore for RecordingMediaObjectStore {
        async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
            let bytes = tokio::fs::read(&input.file_path)
                .await
                .map_err(|_| MediaServiceError::ObjectStoreFailed)?;
            self.written_objects
                .lock()
                .expect("recorded media object writes lock should not be poisoned")
                .push(RecordedMediaObjectWrite {
                    bucket: input.bucket,
                    object_key: input.object_key,
                    content_type: input.content_type,
                    source_path: input.file_path,
                    bytes,
                });

            Ok(())
        }

        async fn get_object(
            &self,
            _input: GetMediaObjectInput,
        ) -> Result<MediaObjectByteStream, MediaServiceError> {
            Err(MediaServiceError::ObjectStoreFailed)
        }

        async fn delete_object(
            &self,
            input: DeleteMediaObjectInput,
        ) -> Result<(), MediaServiceError> {
            self.deleted_objects
                .lock()
                .expect("recorded media object deletes lock should not be poisoned")
                .push(input);
            Ok(())
        }
    }

    #[tokio::test]
    async fn upload_media_route_saves_object_and_metadata_to_postgresql() {
        let store = RecordingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!("media-upload-user-{}@example.com", uuid::Uuid::new_v4());
        let password_hash = hash_password("password123").expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            "media-user",
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
        let boundary = "----occurrence-media-boundary";
        let file_bytes = TEST_JPEG_BYTES;
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"sample.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(file_bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let media_id = body_json["media_id"]
            .as_str()
            .expect("media_id should be returned");
        assert_eq!(
            body_json["media_uri"],
            format!("http://127.0.0.1:3000/media/{media_id}")
        );
        assert_eq!(body_json["bucket"], "occurrence-media");
        assert_eq!(body_json["object_key"], format!("media/{media_id}"));
        assert_eq!(body_json["content_type"], "image/jpeg");
        assert_eq!(body_json["size_bytes"], file_bytes.len() as i64);
        assert_eq!(body_json["original_filename"], "sample.jpg");

        let writes = store
            .written_objects
            .lock()
            .expect("recorded media object writes lock should not be poisoned");

        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].bucket, "occurrence-media");
        assert_eq!(writes[0].object_key, format!("media/{media_id}"));
        assert_eq!(writes[0].content_type, "image/jpeg");
        assert_eq!(writes[0].bytes, file_bytes);
        assert!(
            !writes[0].source_path.exists(),
            "temporary upload file must be removed before the response is returned"
        );
        drop(writes);

        let media_id = uuid::Uuid::parse_str(media_id).expect("media_id should be a UUID");
        // HTTPレスポンスだけではなくDBを直接読み、app -> handler -> service -> repositoryの接続を確認する。
        let metadata = sqlx::query_as::<
            _,
            (
                uuid::Uuid,
                String,
                String,
                String,
                i64,
                Option<String>,
                uuid::Uuid,
                chrono::DateTime<chrono::Utc>,
            ),
        >(
            r#"
            SELECT id, bucket, object_key, content_type, size_bytes,
                   original_filename, uploaded_by, created_at
            FROM media_objects
            WHERE id = $1
            "#,
        )
        .bind(media_id)
        .fetch_one(&db)
        .await
        .expect("app upload metadata should be saved to PostgreSQL");

        assert_eq!(metadata.0, media_id);
        assert_eq!(metadata.1, "occurrence-media");
        assert_eq!(metadata.2, format!("media/{media_id}"));
        assert_eq!(metadata.3, "image/jpeg");
        assert_eq!(metadata.4, file_bytes.len() as i64);
        assert_eq!(metadata.5.as_deref(), Some("sample.jpg"));
        assert_eq!(metadata.6, user_id);
        assert!(metadata.7 <= chrono::Utc::now());

        // 外部キー制約に従い、子レコードから削除してテストデータを残さない。
        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("app upload metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .expect("media app test session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .expect("media app test user should be removed after test");
    }

    #[tokio::test]
    async fn upload_media_route_accepts_body_larger_than_axum_default_limit() {
        let store = RecordingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let db = state.posgre.clone();
        let (user_id, session_token) = create_media_test_session(&db, "large-media-user").await;
        let app = build_app(state);

        let boundary = "----occurrence-large-streaming-boundary";
        let mut file_bytes = vec![0_u8; 3 * 1024 * 1024];
        file_bytes[..TEST_JPEG_BYTES.len()].copy_from_slice(TEST_JPEG_BYTES);
        let body = multipart_file_body(boundary, "large.jpg", "image/jpeg", &file_bytes);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let response_json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        let media_id = uuid::Uuid::parse_str(
            response_json["media_id"]
                .as_str()
                .expect("media_id should be returned"),
        )
        .expect("media_id should be a UUID");

        let writes = store
            .written_objects
            .lock()
            .expect("recorded media writes lock should not be poisoned");
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].bytes.len(), file_bytes.len());
        assert!(!writes[0].source_path.exists());
        drop(writes);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn upload_media_route_removes_temporary_file_after_upload() {
        let store = FailingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let db = state.posgre.clone();
        let (user_id, session_token) = create_media_test_session(&db, "failed-media-user").await;
        let app = build_app(state);
        let boundary = "----occurrence-failed-streaming-boundary";
        let body = multipart_file_body(boundary, "failed.jpg", "image/jpeg", TEST_JPEG_BYTES);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let attempted_paths = store
            .attempted_paths
            .lock()
            .expect("failed media store paths lock should not be poisoned");
        assert_eq!(attempted_paths.len(), 1);
        assert!(
            !attempted_paths[0].exists(),
            "temporary file must be removed after object store failure"
        );
        drop(attempted_paths);

        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn get_media_route_returns_object_stream_for_owner() {
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let store = ReadableAppMediaObjectStore {
            expected_bucket: "occurrence-media".to_string(),
            expected_object_key: object_key.clone(),
            bytes: Arc::new(TEST_JPEG_BYTES.to_vec()),
        };
        let state = test_state_with_media_object_store(Arc::new(store));
        let db = state.posgre.clone();
        let (user_id, session_token) = create_media_test_session(&db, "get-media-owner").await;
        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("owner-photo.jpg")
        .bind(user_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            response.headers()[CONTENT_LENGTH],
            TEST_JPEG_BYTES.len().to_string()
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), TEST_JPEG_BYTES);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .expect("media owner session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .expect("media owner should be removed after test");
    }

    #[tokio::test]
    async fn get_media_route_returns_not_found_for_non_owner() {
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let store = ReadableAppMediaObjectStore {
            expected_bucket: "occurrence-media".to_string(),
            expected_object_key: object_key.clone(),
            bytes: Arc::new(TEST_JPEG_BYTES.to_vec()),
        };
        let state = test_state_with_media_object_store(Arc::new(store));
        let db = state.posgre.clone();
        let (owner_id, _) = create_media_test_session(&db, "get-media-owner").await;
        let (other_user_id, other_session_token) =
            create_media_test_session(&db, "get-media-non-owner").await;

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("private-owner-photo.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        // 別ユーザーの有効なsessionを使い、未認証ではなく所有権判定による拒否を確認する。
        // 404を返すことで、media_idの存在自体も所有者以外へ開示しない。
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .header(COOKIE, format!("session={other_session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_ne!(body.as_ref(), TEST_JPEG_BYTES);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id IN ($1, $2)")
            .bind(owner_id)
            .bind(other_user_id)
            .execute(&db)
            .await
            .expect("media test sessions should be removed after test");
        sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
            .bind(owner_id)
            .bind(other_user_id)
            .execute(&db)
            .await
            .expect("media test users should be removed after test");
    }

    #[tokio::test]
    async fn get_media_route_allows_anonymous_access_when_linked_from_public_occurrence() {
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let media_uri = format!("http://127.0.0.1:3000/media/{media_id}");
        let occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            uuid::Uuid::new_v4()
        );
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let occurrence_nquads = format!(
            "<{occurrence_uri}> <http://rs.tdwg.org/ac/terms/associatedMedia> <{media_uri}> <{graph_uri}> .
             <{occurrence_uri}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <{graph_uri}> .
"
        );

        let occurrence_store = FakeOccurrenceRdfStore::default();
        occurrence_store.insert_occurrence_nquads(&occurrence_uri, occurrence_nquads.into_bytes());
        let media_store = ReadableAppMediaObjectStore {
            expected_bucket: "occurrence-media".to_string(),
            expected_object_key: object_key.clone(),
            bytes: Arc::new(TEST_JPEG_BYTES.to_vec()),
        };
        let mut state = test_state_with_media_object_store(Arc::new(media_store));
        state.occurrence_rdf_store = Arc::new(occurrence_store);
        let db = state.posgre.clone();
        let (owner_id, _) = create_media_test_session(&db, "public-occurrence-media-owner").await;

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("public-occurrence-photo.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        // No session cookie is sent. Access must be granted only because a public
        // occurrence references this media URI in RDF.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            response.headers()[CONTENT_LENGTH],
            TEST_JPEG_BYTES.len().to_string()
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), TEST_JPEG_BYTES);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner should be removed after test");
    }

    #[tokio::test]
    async fn get_media_route_hides_private_occurrence_media_from_anonymous_user() {
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let media_uri = format!("http://127.0.0.1:3000/media/{media_id}");
        let occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            uuid::Uuid::new_v4()
        );
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let occurrence_nquads = format!(
            "<{occurrence_uri}> <http://rs.tdwg.org/ac/terms/associatedMedia> <{media_uri}> <{graph_uri}> .
             <{occurrence_uri}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <{graph_uri}> .
"
        );

        let occurrence_store = FakeOccurrenceRdfStore::default();
        occurrence_store.insert_occurrence_nquads(&occurrence_uri, occurrence_nquads.into_bytes());
        let media_store = ReadableAppMediaObjectStore {
            expected_bucket: "occurrence-media".to_string(),
            expected_object_key: object_key.clone(),
            bytes: Arc::new(TEST_JPEG_BYTES.to_vec()),
        };
        let mut state = test_state_with_media_object_store(Arc::new(media_store));
        state.occurrence_rdf_store = Arc::new(occurrence_store);
        let db = state.posgre.clone();
        let (owner_id, _) = create_media_test_session(&db, "private-occurrence-media-owner").await;

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("private-occurrence-photo.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        // No cookie is supplied. A private RDF reference must not grant anonymous
        // access or disclose whether the media object exists.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_ne!(body.as_ref(), TEST_JPEG_BYTES);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner should be removed after test");
    }

    #[tokio::test]
    async fn get_media_route_hides_private_occurrence_media_from_logged_in_non_owner() {
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let media_uri = format!("http://127.0.0.1:3000/media/{media_id}");
        let occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            uuid::Uuid::new_v4()
        );
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let occurrence_nquads = format!(
            "<{occurrence_uri}> <http://rs.tdwg.org/ac/terms/associatedMedia> <{media_uri}> <{graph_uri}> .
             <{occurrence_uri}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <{graph_uri}> .
"
        );

        let occurrence_store = FakeOccurrenceRdfStore::default();
        occurrence_store.insert_occurrence_nquads(&occurrence_uri, occurrence_nquads.into_bytes());
        let media_store = ReadableAppMediaObjectStore {
            expected_bucket: "occurrence-media".to_string(),
            expected_object_key: object_key.clone(),
            bytes: Arc::new(TEST_JPEG_BYTES.to_vec()),
        };
        let mut state = test_state_with_media_object_store(Arc::new(media_store));
        state.occurrence_rdf_store = Arc::new(occurrence_store);
        let db = state.posgre.clone();
        let (owner_id, _) =
            create_media_test_session(&db, "private-media-owner-for-other-user").await;
        let (viewer_id, viewer_session_token) =
            create_media_test_session(&db, "private-media-non-owner-viewer").await;

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("private-photo-owned-by-another-user.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        // The session is valid but belongs to neither the media uploader nor the
        // private occurrence owner, so the media must remain undisclosed.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .header(COOKIE, format!("session={viewer_session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_ne!(body.as_ref(), TEST_JPEG_BYTES);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id IN ($1, $2)")
            .bind(owner_id)
            .bind(viewer_id)
            .execute(&db)
            .await
            .expect("media test sessions should be removed after test");
        sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
            .bind(owner_id)
            .bind(viewer_id)
            .execute(&db)
            .await
            .expect("media test users should be removed after test");
    }

    #[tokio::test]
    async fn get_media_route_allows_logged_in_non_owner_when_linked_from_public_occurrence() {
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");
        let media_uri = format!("http://127.0.0.1:3000/media/{media_id}");
        let occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            uuid::Uuid::new_v4()
        );
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let occurrence_nquads = format!(
            "<{occurrence_uri}> <http://rs.tdwg.org/ac/terms/associatedMedia> <{media_uri}> <{graph_uri}> .
             <{occurrence_uri}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <{graph_uri}> .
"
        );

        let occurrence_store = FakeOccurrenceRdfStore::default();
        occurrence_store.insert_occurrence_nquads(&occurrence_uri, occurrence_nquads.into_bytes());
        let media_store = ReadableAppMediaObjectStore {
            expected_bucket: "occurrence-media".to_string(),
            expected_object_key: object_key.clone(),
            bytes: Arc::new(TEST_JPEG_BYTES.to_vec()),
        };
        let mut state = test_state_with_media_object_store(Arc::new(media_store));
        state.occurrence_rdf_store = Arc::new(occurrence_store);
        let db = state.posgre.clone();
        let (owner_id, _) =
            create_media_test_session(&db, "public-media-owner-for-other-user").await;
        let (viewer_id, viewer_session_token) =
            create_media_test_session(&db, "public-media-viewer").await;

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("public-photo-owned-by-another-user.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        // The valid session belongs to a different user. Access therefore proves
        // that public RDF linkage, rather than ownership, grants the read.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .header(COOKIE, format!("session={viewer_session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            response.headers()[CONTENT_LENGTH],
            TEST_JPEG_BYTES.len().to_string()
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), TEST_JPEG_BYTES);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id IN ($1, $2)")
            .bind(owner_id)
            .bind(viewer_id)
            .execute(&db)
            .await
            .expect("media test sessions should be removed after test");
        sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
            .bind(owner_id)
            .bind(viewer_id)
            .execute(&db)
            .await
            .expect("media test users should be removed after test");
    }

    #[tokio::test]
    async fn delete_media_route_deletes_owned_media_object_and_metadata() {
        let store = RecordingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let db = state.posgre.clone();
        let (owner_id, session_token) =
            create_media_test_session(&db, "delete-media-route-owner").await;
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("delete-through-app.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/media/{media_id}"))
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["deleted"], true);

        let deletes = store
            .deleted_objects
            .lock()
            .expect("recorded media object deletes lock should not be poisoned");
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].bucket, "occurrence-media");
        assert_eq!(deletes[0].object_key, object_key);
        drop(deletes);

        let metadata_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM media_objects WHERE id = $1")
                .bind(media_id)
                .fetch_one(&db)
                .await
                .expect("media metadata count should be queryable");
        assert_eq!(metadata_count, 0);

        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner should be removed after test");
    }

    #[tokio::test]
    async fn delete_media_route_rejects_non_owner_and_preserves_object_and_metadata() {
        let store = RecordingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let db = state.posgre.clone();
        let (owner_id, _) = create_media_test_session(&db, "delete-media-owner").await;
        let (other_user_id, other_session_token) =
            create_media_test_session(&db, "delete-media-non-owner").await;
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("non-owner-delete-target.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/media/{media_id}"))
                    .header(COOKIE, format!("session={other_session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let deletes = store
            .deleted_objects
            .lock()
            .expect("recorded media object deletes lock should not be poisoned");
        assert!(
            deletes.is_empty(),
            "non-owner request must not reach Garage deletion"
        );
        drop(deletes);

        let metadata_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM media_objects WHERE id = $1")
                .bind(media_id)
                .fetch_one(&db)
                .await
                .expect("media metadata count should be queryable");
        assert_eq!(
            metadata_count, 1,
            "non-owner request must preserve metadata"
        );

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id IN ($1, $2)")
            .bind(owner_id)
            .bind(other_user_id)
            .execute(&db)
            .await
            .expect("media test sessions should be removed after test");
        sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
            .bind(owner_id)
            .bind(other_user_id)
            .execute(&db)
            .await
            .expect("media test users should be removed after test");
    }

    #[tokio::test]
    async fn delete_media_route_requires_login_and_preserves_object_and_metadata() {
        let store = RecordingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let db = state.posgre.clone();
        let (owner_id, _) =
            create_media_test_session(&db, "delete-media-unauthenticated-owner").await;
        let media_id = uuid::Uuid::new_v4();
        let object_key = format!("media/{media_id}");

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("unauthenticated-delete-target.jpg")
        .bind(owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("media metadata should be inserted");
        let app = build_app(state);

        // Intentionally omit the session cookie. Authentication must fail before
        // ownership lookup or either persistent store is modified.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/media/{media_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let deletes = store
            .deleted_objects
            .lock()
            .expect("recorded media object deletes lock should not be poisoned");
        assert!(
            deletes.is_empty(),
            "unauthenticated request must not reach Garage deletion"
        );
        drop(deletes);

        let metadata_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM media_objects WHERE id = $1")
                .bind(media_id)
                .fetch_one(&db)
                .await
                .expect("media metadata count should be queryable");
        assert_eq!(
            metadata_count, 1,
            "unauthenticated request must preserve metadata"
        );

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("media owner should be removed after test");
    }

    #[tokio::test]
    async fn upload_media_route_without_session_returns_unauthorized_and_does_not_write_object() {
        let store = RecordingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let app = build_app(state);

        let boundary = "----occurrence-unauthorized-media-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"sample.jpg\"\r\nContent-Type: image/jpeg\r\n\r\nfake-jpeg-bytes\r\n--{boundary}--\r\n"
        );

        // Cookieを意図的に付けず、routeから認証処理を通して401になることを確認する。
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // 認証に失敗したrequestがobject storageへ到達していないことも保証する。
        let writes = store
            .written_objects
            .lock()
            .expect("recorded media object writes lock should not be poisoned");
        assert!(writes.is_empty());
    }

    fn required_garage_env(key: &str) -> String {
        std::env::var(key)
            .unwrap_or_else(|_| panic!("{} must be set for real Garage app test", key))
    }

    fn run_aws_s3_command_for_real_garage(args: &[String]) {
        let endpoint = required_garage_env("S3_ENDPOINT");
        let region = required_garage_env("S3_REGION");
        let access_key = required_garage_env("S3_ACCESS_KEY");
        let secret_key = required_garage_env("S3_SECRET_KEY");

        let output = Command::new("aws")
            .env("AWS_ACCESS_KEY_ID", access_key)
            .env("AWS_SECRET_ACCESS_KEY", secret_key)
            .env("AWS_DEFAULT_REGION", region)
            .arg("--endpoint-url")
            .arg(endpoint)
            .args(args)
            .output()
            .expect("aws CLI should be installed for real Garage app test");

        assert!(
            output.status.success(),
            "aws command failed: status={:?}, stdout={}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    #[ignore = "requires a running Garage server, backend/.env S3_* credentials, and PostgreSQL"]
    async fn upload_media_route_writes_object_to_real_garage() {
        dotenvy::dotenv().ok();

        let bucket = required_garage_env("S3_BUCKET");
        let media_object_store = Arc::new(
            GarageMediaObjectStore::from_env()
                .expect("Garage object store should be created from backend/.env S3 settings"),
        );
        let state = test_state_with_media_object_store(media_object_store);
        let db = state.posgre.clone();

        let email = format!(
            "real-garage-media-upload-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let password_hash = hash_password("password123").expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            "real-garage-media-user",
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
        let boundary = "----occurrence-real-garage-media-boundary";
        let file_bytes = TEST_JPEG_BYTES;
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"real-garage.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(file_bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let object_key = body_json["object_key"]
            .as_str()
            .expect("object_key should be returned");
        let object_uri = format!("s3://{bucket}/{object_key}");

        // app routeから本番Garage object storeへ実際に書けたかを、GarageのS3互換APIで確認する。
        run_aws_s3_command_for_real_garage(&vec![
            "s3".to_string(),
            "ls".to_string(),
            object_uri.clone(),
        ]);

        run_aws_s3_command_for_real_garage(&vec!["s3".to_string(), "rm".to_string(), object_uri]);
    }

    #[tokio::test]
    #[ignore = "requires a running Garage server, backend/.env S3_* credentials, and PostgreSQL"]
    async fn get_media_route_reads_object_from_real_garage() {
        dotenvy::dotenv().ok();

        let bucket = required_garage_env("S3_BUCKET");
        let media_object_store = Arc::new(
            GarageMediaObjectStore::from_env()
                .expect("Garage object store should be created from backend/.env S3 settings"),
        );
        let state = test_state_with_media_object_store(media_object_store);
        let db = state.posgre.clone();
        let (user_id, session_token) =
            create_media_test_session(&db, "real-garage-media-get").await;
        let app = build_app(state);
        let boundary = "----occurrence-real-garage-get-boundary";
        let upload_body = multipart_file_body(
            boundary,
            "real-garage-get.jpg",
            "image/jpeg",
            TEST_JPEG_BYTES,
        );

        // Upload through the same public API first so the test covers the actual
        // Garage object key and PostgreSQL metadata used by the GET route.
        let upload_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::from(upload_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(upload_response.status(), StatusCode::CREATED);
        let upload_body = to_bytes(upload_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let upload_json: serde_json::Value = serde_json::from_slice(&upload_body).unwrap();
        let media_id = upload_json["media_id"]
            .as_str()
            .expect("media_id should be returned");
        let object_key = upload_json["object_key"]
            .as_str()
            .expect("object_key should be returned");

        let get_response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(get_response.headers()[CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            get_response.headers()[CONTENT_LENGTH],
            TEST_JPEG_BYTES.len().to_string()
        );
        let downloaded = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(downloaded.as_ref(), TEST_JPEG_BYTES);

        // Remove both physical and metadata records so repeated ignored runs remain independent.
        let object_uri = format!("s3://{bucket}/{object_key}");
        run_aws_s3_command_for_real_garage(&vec!["s3".to_string(), "rm".to_string(), object_uri]);
        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(uuid::Uuid::parse_str(media_id).expect("media_id should be a UUID"))
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .expect("media owner session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .expect("media owner should be removed after test");
    }

    #[tokio::test]
    #[ignore = "requires running Fuseki, Garage, PostgreSQL, and backend/.env integration settings"]
    async fn get_public_occurrence_media_from_real_fuseki_and_real_garage() {
        dotenvy::dotenv().ok();

        let config = Config::from_env().expect("integration configuration should be valid");
        let bucket = config.garage.bucket.clone();
        let db = PgPoolOptions::new()
            .connect_lazy(&config.posgre.url)
            .expect("failed to create lazy database pool");
        let fuseki_store = Arc::new(FusekiClient::new(config.fuseki.clone()));
        let garage_store = Arc::new(
            GarageMediaObjectStore::from_env()
                .expect("Garage object store should be created from backend/.env S3 settings"),
        );
        let state =
            AppState::new_with_media_object_store(config, db.clone(), fuseki_store, garage_store);
        let (owner_id, session_token) =
            create_media_test_session(&db, "real-fuseki-garage-public-media").await;
        let app = build_app(state);

        let boundary = "----occurrence-real-fuseki-garage-boundary";
        let upload_body =
            multipart_file_body(boundary, "public-real.jpg", "image/jpeg", TEST_JPEG_BYTES);
        let upload_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::from(upload_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(upload_response.status(), StatusCode::CREATED);
        let upload_body = to_bytes(upload_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let upload_json: serde_json::Value = serde_json::from_slice(&upload_body).unwrap();
        let media_id = upload_json["media_id"]
            .as_str()
            .expect("media_id should be returned")
            .to_string();
        let media_uri = upload_json["media_uri"]
            .as_str()
            .expect("media_uri should be returned")
            .to_string();
        let object_key = upload_json["object_key"]
            .as_str()
            .expect("object_key should be returned")
            .to_string();

        // Register through the public API so Fuseki receives backend-managed
        // creator, timestamps, and the default public accessRights statement.
        let occurrence_nquads = format!(
            "_:occurrence <http://rs.tdwg.org/ac/terms/associatedMedia> <{media_uri}> <https://bio-database.net/graphs/occurrences> .
"
        );
        let occurrence_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::from(occurrence_nquads))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(occurrence_response.status(), StatusCode::CREATED);
        let occurrence_body = to_bytes(occurrence_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let occurrence_json: serde_json::Value = serde_json::from_slice(&occurrence_body).unwrap();
        let occurrence_id = occurrence_json["occurrence_id"]
            .as_str()
            .expect("occurrence_id should be returned")
            .to_string();

        // No session is supplied. Fuseki's public reference must authorize the
        // Garage stream through the same GET route used by frontend clients.
        let get_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/media/{media_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(get_response.headers()[CONTENT_TYPE], "image/jpeg");
        let downloaded = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(downloaded.as_ref(), TEST_JPEG_BYTES);

        let delete_occurrence_response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/occurrences/{occurrence_id}"))
                    .header(COOKIE, format!("session={session_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_occurrence_response.status(), StatusCode::OK);

        run_aws_s3_command_for_real_garage(&vec![
            "s3".to_string(),
            "rm".to_string(),
            format!("s3://{bucket}/{object_key}"),
        ]);
        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&media_id).expect("media_id should be a UUID"))
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("owner session should be removed after test");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(owner_id)
            .execute(&db)
            .await
            .expect("owner should be removed after test");
    }

    #[tokio::test]
    async fn upload_media_route_rejects_payload_larger_than_global_limit_and_does_not_write_object()
    {
        let store = RecordingMediaObjectStore::default();
        let state = test_state_with_media_object_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "media-upload-large-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let password_hash = hash_password("password123").expect("password should be hashed");

        let user_id = sqlx::query_scalar!(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id
            "#,
            email,
            "media-user",
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
        let boundary = "----occurrence-media-boundary";
        let file_bytes = b"small-body-but-content-length-is-larger-than-global-limit";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"large.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n{}\r\n--{boundary}--\r\n",
            std::str::from_utf8(file_bytes).expect("test bytes should be utf8")
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/media")
                    .header(
                        CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header(
                        header::CONTENT_LENGTH,
                        (super::MEDIA_REQUEST_BODY_LIMIT_BYTES + 1).to_string(),
                    )
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let writes = store
            .written_objects
            .lock()
            .expect("recorded media object writes lock should not be poisoned");

        assert!(
            writes.is_empty(),
            "oversized media must not be written to object storage"
        );
    }

    #[tokio::test]
    async fn reset_password_route_updates_password_for_valid_token() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!("route-reset-complete-{}@example.com", uuid::Uuid::new_v4());
        let old_password_hash =
            hash_password("old-password-123").expect("old password hash should be created");

        AuthRepository::create_user(&db, &email, "reset-user", &old_password_hash)
            .await
            .expect("user should be created");

        let user = AuthRepository::find_user_by_email(&db, &email)
            .await
            .expect("user query should succeed")
            .expect("user should exist");

        let token = uuid::Uuid::new_v4().to_string();
        let token_hash = hash_token(&token);

        // app経由テストでは、HTTP handlerからAuthService::reset_passwordへつながり、
        // token hash照合で対象ユーザーのpassword hashが更新されることを確認する。
        AuthRepository::upsert_password_reset_token(&db, user.id, &token_hash)
            .await
            .expect("password reset token should be stored");

        let body = serde_json::json!({
            "token": token,
            "password": "new-password-123"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/reset_password")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let updated_user = AuthRepository::find_user_by_email(&db, &email)
            .await
            .expect("updated user query should succeed")
            .expect("updated user should exist");

        assert_ne!(updated_user.password_hash, old_password_hash);
        assert_ne!(updated_user.password_hash, "new-password-123");

        let used_at: (Option<chrono::DateTime<chrono::Utc>>,) = sqlx::query_as(
            r#"
            SELECT used_at
            FROM password_reset_tokens
            WHERE token_hash = $1
            "#,
        )
        .bind(&token_hash)
        .fetch_one(&db)
        .await
        .expect("password reset token should still exist");

        assert!(used_at.0.is_some(), "reset token should be marked used");
    }

    #[tokio::test]
    async fn request_password_reset_route_returns_success_like_response_for_unregistered_email() {
        let state = test_state();
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!(
            "missing-password-reset-{}@example.com",
            uuid::Uuid::new_v4()
        );

        delete_mailpit_messages().await;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/request_password_reset")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"email":"{}"}}"#, email)))
                    .unwrap(),
            )
            .await
            .unwrap();

        // 未登録emailでも登録済みemailと同じ成功風レスポンスにする。
        // ここで401/404を返すと、攻撃者がメールアドレスの登録有無を推測できる。
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["message"], "password reset mail sent");

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
        .expect("password reset tokens should be queryable");

        assert_eq!(token_hash_count.0, 0);

        let mailpit_messages = fetch_mailpit_messages().await;
        assert!(
            mailpit_messages.is_empty(),
            "password reset email should not be sent for unregistered email"
        );
    }

    #[tokio::test]
    #[ignore = "sends a real password reset email through the configured production SMTP server"]
    async fn request_password_reset_route_sends_real_email_to_gmail_for_temporary_user() {
        dotenvy::dotenv().ok();

        let database_url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for real password reset mail test");
        let smtp_host = std::env::var("SMTP_HOST")
            .expect("SMTP_HOST must be set to a real SMTP server for this ignored test");

        // このテストは実メール送信確認用なので、Mailpit/localhost設定では成功扱いにしない。
        // Mailpit経路は通常appテストで確認済み。本テストはResend等の本番SMTP疎通だけを見る。
        assert!(
            smtp_host != "127.0.0.1" && smtp_host != "localhost",
            "real email test requires a non-local SMTP_HOST"
        );

        let smtp_port = std::env::var("SMTP_PORT")
            .expect("SMTP_PORT must be set for real password reset mail test")
            .parse::<u16>()
            .expect("SMTP_PORT must be a valid u16");
        let smtp_username = std::env::var("SMTP_USERNAME")
            .expect("SMTP_USERNAME must be set for real password reset mail test");
        let smtp_password = std::env::var("SMTP_PASSWORD")
            .expect("SMTP_PASSWORD must be set for real password reset mail test");
        let smtp_tls = std::env::var("SMTP_TLS").unwrap_or_else(|_| "starttls".to_string());
        let mail_from = std::env::var("MAIL_FROM")
            .expect("MAIL_FROM must be set for real password reset mail test");
        // 到達率確認のため、このignoredテストでは一時的に独自ドメインのURLを本文に入れる。
        let app_base_url = "https://bio-database.net".to_string();

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url,
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
            },
            smtp: SmtpConfig {
                host: smtp_host,
                port: smtp_port,
                username: smtp_username,
                password: smtp_password,
                tls: smtp_tls,
                from: mail_from,
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

        let state = AppState::new(config, posgre, Arc::new(NoopOccurrenceRdfStore));
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = "test@gmail.com";
        let password_hash = hash_password("temporary-password-123")
            .expect("temporary password hash should be created");

        // 実メール送信用に対象emailのユーザーがなければ仮作成する。
        // 既に同じemailのユーザーがいる場合は上書きせず、そのユーザーに対するreset mailだけ送る。
        sqlx::query(
            r#"
            INSERT INTO users (email, user_name, password_hash)
            VALUES ($1, $2, $3)
            ON CONFLICT (email) DO NOTHING
            "#,
        )
        .bind(email)
        .bind("real-mail-reset-test")
        .bind(&password_hash)
        .execute(&db)
        .await
        .expect("temporary user should be present for real password reset mail test");

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/request_password_reset")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(r#"{{"email":"{}"}}"#, email)))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

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
        .bind(email)
        .fetch_one(&db)
        .await
        .expect("password reset token should be queryable");

        assert_eq!(token_hash_count.0, 1);
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

        AuthRepository::create_pending_registration(&db, &email, &token_hash)
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

        AuthRepository::create_pending_registration(&db, &email, &token_hash)
            .await
            .expect("pending registration should be created");

        AuthRepository::create_user(
            &db,
            &email,
            "existing_user",
            "$argon2id$dummy-existing-password-hash",
        )
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
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

        assert!(
            set_cookie.contains("Max-Age=604800"),
            "session cookie should live for 7 days"
        );
    }

    #[tokio::test]
    async fn login_route_sets_secure_session_cookie_when_cookie_secure_enabled() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for app tests");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: true,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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

        let state = AppState::new(config, posgre, Arc::new(NoopOccurrenceRdfStore));
        let db = state.posgre.clone();
        let app = build_app(state);

        let email = format!(
            "route-login-secure-cookie-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let password = "password123";
        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
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
            set_cookie.contains("Secure"),
            "session cookie should be Secure when cookie_secure is enabled"
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
            .await
            .expect("user should be created");

        let login_output = AuthService::login(&db, email, password.to_string())
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let password_hash = hash_password(password).expect("password hash should be created");

        AuthRepository::create_user(&db, &email, "saku", &password_hash)
            .await
            .expect("user should be created");

        let login_output = AuthService::login(&db, email, password.to_string())
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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
        let password_hash = hash_password("password123").expect("password should be hashed");

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
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        let occurrence_id = body["occurrence_id"]
            .as_str()
            .expect("occurrence_id should be string");

        let occurrence_uri = body["occurrence_uri"]
            .as_str()
            .expect("occurrence_uri should be string");

        assert!(occurrence_uri.starts_with("https://bio-database.net/occurrences/"));

        assert!(
            occurrence_uri.ends_with(occurrence_id),
            "occurrence_uri should contain occurrence_id"
        );

        uuid::Uuid::parse_str(occurrence_id).expect("occurrence_id should be valid UUID");
    }

    #[tokio::test]
    async fn create_occurrence_route_with_valid_session_saves_nquads_to_store() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!("occurrence-store-user-{}@example.com", uuid::Uuid::new_v4());
        let user_name = "occurrence-store-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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
            5,
            "saved N-Quads should contain frontend quad plus backend creator, created, modified, and accessRights quads"
        );

        let expected_subject = format!("<{}>", occurrence_uri);

        assert!(
            parsed_quads
                .iter()
                .all(|quad| { quad.subject.to_string() == expected_subject }),
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

        let expected_creator_object = format!("<https://bio-database.net/users/{}>", user_id);

        let has_creator_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string() == expected_creator_object
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_creator_quad,
            "saved N-Quads should contain backend-confirmed creator user URI"
        );

        let has_created_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/created>"
                && quad
                    .object
                    .to_string()
                    .contains("^^<http://www.w3.org/2001/XMLSchema#dateTime>")
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_created_quad,
            "saved N-Quads should contain backend-created timestamp as xsd:dateTime"
        );

        let has_modified_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/modified>"
                && quad
                    .object
                    .to_string()
                    .contains("^^<http://www.w3.org/2001/XMLSchema#dateTime>")
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_modified_quad,
            "saved N-Quads should contain backend-modified timestamp as xsd:dateTime"
        );

        let has_access_rights_quad = parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/accessRights>"
                && quad.object.to_string()
                    == "<https://bio-database.net/terms/access-rights/public>"
                && quad.graph_name.to_string() == "<https://bio-database.net/graphs/occurrences>"
        });

        assert!(
            has_access_rights_quad,
            "saved N-Quads should default missing accessRights to public"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_media_owned_by_another_user_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();
        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();
        let (request_user_id, request_session_token) =
            create_media_test_session(&db, "occurrence-media-request-user").await;
        let (media_owner_id, _) =
            create_media_test_session(&db, "occurrence-media-other-owner").await;
        let media_id = uuid::Uuid::new_v4();
        let media_uri = format!("http://127.0.0.1:3000/media/{media_id}");
        let object_key = format!("media/{media_id}");

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("other-users-photo.jpg")
        .bind(media_owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("other user's media metadata should be inserted");

        let frontend_nquads = format!(
            "_:occurrence <http://rs.tdwg.org/ac/terms/associatedMedia> <{media_uri}> <https://bio-database.net/graphs/occurrences> .
"
        );
        let app = build_app(state);

        // The media exists, but uploaded_by differs from the authenticated occurrence creator.
        // Reject before RDF persistence so another user's object cannot be exposed by a new occurrence.
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences")
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={request_session_token}"))
                    .body(Body::from(frontend_nquads))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");
        assert!(
            saved.is_empty(),
            "occurrence containing another user's media URI must not be saved"
        );
        drop(saved);

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id IN ($1, $2)")
            .bind(request_user_id)
            .bind(media_owner_id)
            .execute(&db)
            .await
            .expect("test sessions should be removed");
        sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
            .bind(request_user_id)
            .bind(media_owner_id)
            .execute(&db)
            .await
            .expect("test users should be removed");
    }

    #[tokio::test]
    async fn create_occurrence_route_with_invalid_nquads_returns_bad_request_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-invalid-rdf-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-invalid-rdf-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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
    async fn create_occurrence_route_rejects_invalid_access_rights_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-invalid-access-rights-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-invalid-access-rights-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let cases = [
            (
                "literal accessRights",
                br#"
    _:occurrence <http://purl.org/dc/terms/accessRights> "public" <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
            (
                "unknown accessRights URI",
                br#"
    _:occurrence <http://purl.org/dc/terms/accessRights> <https://example.org/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
            (
                "multiple accessRights",
                br#"
    _:occurrence <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
    _:occurrence <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
        ];

        for (case_name, frontend_nquads) in cases {
            let response = app
                .clone()
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

            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{case_name} should return 400"
            );

            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

            let body_json: serde_json::Value =
                serde_json::from_slice(&body).expect("response body should be JSON");

            assert_eq!(body_json["error"], "invalid_access_rights");
            assert_eq!(body_json["message"], "Invalid access rights");
        }

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "invalid accessRights requests should not be saved to OccurrenceRdfStore"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_when_rdf_store_fails_returns_bad_gateway() {
        let store = FailingOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-rdf-store-fail-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-rdf-store-fail-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-reject-creator-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-reject-creator-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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
    async fn create_occurrence_route_rejects_frontend_created_or_modified_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-reject-managed-time-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-reject-managed-time-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let cases = [
            (
                "created",
                br#"
    _:occurrence <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    _:occurrence <http://purl.org/dc/terms/created> "2026-06-01T12:34:56Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
            (
                "modified",
                br#"
    _:occurrence <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    _:occurrence <http://purl.org/dc/terms/modified> "2026-06-01T12:34:56Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
        ];

        for (predicate_name, frontend_nquads) in cases {
            let response = app
                .clone()
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

            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "frontend-supplied {predicate_name} should return 400"
            );

            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

            let body_json: serde_json::Value =
                serde_json::from_slice(&body).expect("response body should be JSON");

            assert_eq!(body_json["error"], "forbidden_rdf_predicate");
            assert_eq!(
                body_json["message"],
                "Frontend RDF must not contain backend-managed predicates"
            );
        }

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "RDF containing frontend-supplied created or modified should not be saved"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_non_occurrence_graph_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-wrong-graph-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-wrong-graph-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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
    async fn create_occurrence_route_rejects_missing_graph_name_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-missing-graph-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-missing-graph-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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
    _:occurrence <https://example.org/vocab/scientificName> "Lumbricus terrestris" .
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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

        assert_eq!(saved.len(), 0, "RDF without graph name should not be saved");
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_invalid_blank_node_subject_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-invalid-subject-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-invalid-subject-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let cases = [
            (
                "named node subject",
                br#"
    <https://evil.example/fake-occurrence> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
            (
                "multiple blank node subjects",
                br#"
    _:occurrence_a <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    _:occurrence_b <https://example.org/vocab/locality> "somewhere" <https://bio-database.net/graphs/occurrences> .
    "# as &[u8],
            ),
        ];

        for (case_name, frontend_nquads) in cases {
            let response = app
                .clone()
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

            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{case_name} should return 400"
            );

            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

            let body_json: serde_json::Value =
                serde_json::from_slice(&body).expect("response body should be JSON");

            assert_eq!(body_json["error"], "invalid_blank_node_subject");
            assert_eq!(body_json["message"], "Invalid blank node subject");
        }

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "RDF with invalid blank node subject should not be saved"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_object_blank_node_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-object-blank-node-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-object-blank-node-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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
    _:occurrence <https://example.org/vocab/relatedObject> _:object <https://bio-database.net/graphs/occurrences> .
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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "invalid_object_blank_node");
        assert_eq!(body_json["message"], "Invalid object blank node");

        let saved = store
            .saved_nquads
            .lock()
            .expect("mutex should not be poisoned");

        assert_eq!(
            saved.len(),
            0,
            "RDF with object blank node should not be saved"
        );
    }

    #[tokio::test]
    async fn create_occurrence_route_rejects_empty_rdf_and_does_not_save() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-empty-rdf-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-empty-rdf-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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

        assert_eq!(saved.len(), 0, "empty RDF should not be saved");
    }

    #[tokio::test]
    #[ignore]
    async fn create_occurrence_route_saves_data_to_real_fuseki() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration test");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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

        let state = AppState::new(config.clone(), posgre, Arc::new(fuseki_client));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-real-fuseki-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-real-fuseki-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let scientific_name = format!("Lumbricus terrestris {}", uuid::Uuid::new_v4());

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

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

        let sparql_url = format!("{}/sparql", config.fuseki.base_url.trim_end_matches('/'));

        let ask_response = reqwest::Client::new()
            .post(sparql_url)
            .basic_auth(&config.fuseki.user, Some(&config.fuseki.password))
            .header(reqwest::header::CONTENT_TYPE, "application/sparql-query")
            .header(reqwest::header::ACCEPT, "application/sparql-results+json")
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
    #[ignore]
    async fn update_occurrence_route_replaces_existing_occurrence_in_real_fuseki() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration test");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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
        let state = AppState::new(config, posgre, Arc::new(fuseki_client.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-real-fuseki-update-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-real-fuseki-update-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
        let creator_predicate = "http://purl.org/dc/terms/creator";
        let created_predicate = "http://purl.org/dc/terms/created";
        let modified_predicate = "http://purl.org/dc/terms/modified";
        let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
        let old_scientific_name = format!("Old update target {}", uuid::Uuid::new_v4());
        let new_scientific_name = format!("New update target {}", uuid::Uuid::new_v4());

        let existing_nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
<{}> <{}> <https://bio-database.net/users/{}> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <https://bio-database.net/terms/access-rights/private> <{}> .
"#,
            occurrence_uri,
            scientific_name_predicate,
            old_scientific_name,
            graph_uri,
            occurrence_uri,
            creator_predicate,
            user_id,
            graph_uri,
            occurrence_uri,
            created_predicate,
            graph_uri,
            occurrence_uri,
            modified_predicate,
            graph_uri,
            occurrence_uri,
            access_rights_predicate,
            graph_uri,
        );

        fuseki_client
            .save_nquads(existing_nquads.into_bytes())
            .await
            .expect("existing occurrence should be saved to real Fuseki");

        let app = build_app(state);
        let frontend_nquads = format!(
            r#"_:updated <{}> "{}" <{}> .
_:updated <{}> <https://bio-database.net/terms/access-rights/public> <{}> .
"#,
            scientific_name_predicate,
            new_scientific_name,
            graph_uri,
            access_rights_predicate,
            graph_uri,
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.into_bytes()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let updated_nquads = fuseki_client
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("updated occurrence should be fetched from real Fuseki")
            .expect("updated occurrence should exist in real Fuseki");

        let updated_text = String::from_utf8(updated_nquads)
            .expect("updated N-Quads should be valid UTF-8 for assertions");

        assert!(
            updated_text.contains(&new_scientific_name),
            "updated occurrence should contain new scientificName"
        );
        assert!(
            !updated_text.contains(&old_scientific_name),
            "updated occurrence should not contain old scientificName"
        );
        assert!(
            updated_text.contains(&format!("<https://bio-database.net/users/{}>", user_id)),
            "updated occurrence should preserve creator"
        );
        assert!(
            updated_text
                .contains("\"2026-06-02T10:20:30Z\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"),
            "updated occurrence should preserve created"
        );
        assert!(
            updated_text.contains("<https://bio-database.net/terms/access-rights/public>"),
            "updated occurrence should use frontend accessRights"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn delete_occurrence_route_deletes_existing_occurrence_from_real_fuseki() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration test");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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
        let state = AppState::new(config, posgre, Arc::new(fuseki_client.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-real-fuseki-delete-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-real-fuseki-delete-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
        let creator_predicate = "http://purl.org/dc/terms/creator";
        let created_predicate = "http://purl.org/dc/terms/created";
        let modified_predicate = "http://purl.org/dc/terms/modified";
        let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
        let scientific_name = format!("Delete real Fuseki target {}", uuid::Uuid::new_v4());

        let existing_nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
<{}> <{}> <https://bio-database.net/users/{}> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <https://bio-database.net/terms/access-rights/private> <{}> .
"#,
            occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            occurrence_uri,
            creator_predicate,
            user_id,
            graph_uri,
            occurrence_uri,
            created_predicate,
            graph_uri,
            occurrence_uri,
            modified_predicate,
            graph_uri,
            occurrence_uri,
            access_rights_predicate,
            graph_uri,
        );

        fuseki_client
            .save_nquads(existing_nquads.into_bytes())
            .await
            .expect("existing occurrence should be saved to real Fuseki");

        let saved = fuseki_client
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("saved occurrence should be fetched from real Fuseki");
        assert!(
            saved.is_some(),
            "test precondition: occurrence should exist before DELETE"
        );

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["deleted"], true);

        let deleted = fuseki_client
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("deleted occurrence lookup should be handled by real Fuseki");

        assert!(
            deleted.is_none(),
            "deleted occurrence should not remain in real Fuseki"
        );
    }

    #[tokio::test]
    async fn delete_occurrence_route_with_valid_session_deletes_existing_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-delete-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-delete-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let existing_nquads = format!(
            r#"<{}> <http://rs.tdwg.org/dwc/terms/scientificName> "Delete target" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri, occurrence_uri, user_id, occurrence_uri,
        );

        store.insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.into_bytes());

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["deleted"], true);

        let deleted = store
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("fake store should handle lookup after delete");

        assert!(
            deleted.is_none(),
            "deleted occurrence RDF should be removed from store"
        );
    }

    #[tokio::test]
    async fn delete_occurrence_route_requires_login_and_does_not_delete() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let creator_user_id = uuid::Uuid::new_v4();

        let existing_nquads = format!(
            r#"<{}> <http://rs.tdwg.org/dwc/terms/scientificName> "Delete protected target" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri, occurrence_uri, creator_user_id, occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.clone().into_bytes());

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["error"], "invalid_session");
        assert_eq!(body_json["message"], "Invalid session");

        let stored_nquads = store
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("fake store should return occurrence")
            .expect("occurrence should still exist");

        assert_eq!(
            stored_nquads,
            existing_nquads.as_bytes(),
            "unauthenticated delete attempt should not remove occurrence RDF"
        );
    }

    #[tokio::test]
    async fn delete_occurrence_route_returns_not_found_for_missing_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-delete-missing-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-delete-missing-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let missing_occurrence_id = uuid::Uuid::new_v4();
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/occurrences/{}", missing_occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["error"], "occurrence_not_found");
        assert_eq!(body_json["message"], "Occurrence not found");
    }

    #[tokio::test]
    async fn delete_occurrence_route_when_rdf_store_delete_fails_returns_bad_gateway() {
        let store = DeleteFailingOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-delete-store-failure-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-delete-store-failure-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let existing_nquads = format!(
            r#"<{}> <http://rs.tdwg.org/dwc/terms/scientificName> "Delete store failure target" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri, occurrence_uri, user_id, occurrence_uri,
        );

        store.insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.into_bytes());

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["error"], "rdf_store_error");
        assert_eq!(body_json["message"], "Failed to save occurrence RDF");

        let attempted_delete_uris = store
            .attempted_delete_uris
            .lock()
            .expect("mutex should not be poisoned");
        assert_eq!(attempted_delete_uris.as_slice(), &[occurrence_uri]);
    }

    #[tokio::test]
    async fn delete_occurrence_route_hides_other_users_occurrence_from_editor_and_does_not_delete()
    {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-delete-other-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-delete-other-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

        let deleter_user_id = sqlx::query_scalar!(
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
            deleter_user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let creator_user_id = uuid::Uuid::new_v4();

        let existing_nquads = format!(
            r#"<{}> <http://rs.tdwg.org/dwc/terms/scientificName> "Other user's delete target" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri, occurrence_uri, creator_user_id, occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.clone().into_bytes());

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let stored_nquads = store
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("fake store should return occurrence")
            .expect("occurrence should still exist");

        assert_eq!(
            stored_nquads,
            existing_nquads.as_bytes(),
            "other user's delete attempt should not remove occurrence RDF"
        );
    }

    #[tokio::test]
    async fn update_occurrence_route_requires_login_and_does_not_update() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let creator_user_id = uuid::Uuid::new_v4();

        let existing_nquads = format!(
            r#"<{}> <http://rs.tdwg.org/dwc/terms/scientificName> "Original name" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/created> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/modified> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri,
            occurrence_uri,
            creator_user_id,
            occurrence_uri,
            occurrence_uri,
            occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.clone().into_bytes());

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let app = build_app(state);

        let frontend_nquads = br#"_:updated <http://rs.tdwg.org/dwc/terms/scientificName> "Updated without login" <https://bio-database.net/graphs/occurrences> .
"#;

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(CONTENT_TYPE, "application/n-quads")
                    .body(Body::from(frontend_nquads.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["error"], "invalid_session");
        assert_eq!(body_json["message"], "Invalid session");

        let stored_nquads = store
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("fake store should return occurrence")
            .expect("occurrence should still exist");

        assert_eq!(
            stored_nquads,
            existing_nquads.as_bytes(),
            "unauthenticated update attempt should not replace occurrence RDF"
        );
    }

    #[tokio::test]
    async fn update_occurrence_route_hides_other_users_occurrence_from_editor_and_does_not_update()
    {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-update-other-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-update-other-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

        let updater_user_id = sqlx::query_scalar!(
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
        .expect("updater user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            updater_user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let owner_user_id = uuid::Uuid::new_v4();

        let existing_nquads = format!(
            r#"<{}> <http://rs.tdwg.org/dwc/terms/scientificName> "Original name" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/created> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/modified> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri,
            occurrence_uri,
            owner_user_id,
            occurrence_uri,
            occurrence_uri,
            occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.clone().into_bytes());

        let frontend_nquads = br#"_:updated <http://rs.tdwg.org/dwc/terms/scientificName> "Updated by other user" <https://bio-database.net/graphs/occurrences> .
"#;

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.to_vec()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let stored_nquads = store
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("fake store should return occurrence")
            .expect("occurrence should still exist");

        assert_eq!(
            stored_nquads,
            existing_nquads.as_bytes(),
            "other user's update attempt should not replace occurrence RDF"
        );
    }

    #[tokio::test]
    async fn update_occurrence_route_rejects_media_owned_by_another_user_and_does_not_update() {
        let store = FakeOccurrenceRdfStore::default();
        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();
        let (occurrence_owner_id, occurrence_owner_session) =
            create_media_test_session(&db, "occurrence-update-media-owner").await;
        let (media_owner_id, _) =
            create_media_test_session(&db, "occurrence-update-other-media-owner").await;
        let media_id = uuid::Uuid::new_v4();
        let media_uri = format!("http://127.0.0.1:3000/media/{media_id}");
        let object_key = format!("media/{media_id}");

        sqlx::query(
            r#"
            INSERT INTO media_objects (
                id, bucket, object_key, content_type, size_bytes,
                original_filename, uploaded_by, sha256
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(media_id)
        .bind("occurrence-media")
        .bind(&object_key)
        .bind("image/jpeg")
        .bind(TEST_JPEG_BYTES.len() as i64)
        .bind("other-users-update-photo.jpg")
        .bind(media_owner_id)
        .bind(hex::encode(sha2::Sha256::digest(TEST_JPEG_BYTES)))
        .execute(&db)
        .await
        .expect("other user's media metadata should be inserted");

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{occurrence_id}");
        let existing_nquads = format!(
            r#"<{occurrence_uri}> <http://rs.tdwg.org/dwc/terms/scientificName> "Original name" <https://bio-database.net/graphs/occurrences> .
<{occurrence_uri}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{occurrence_owner_id}> <https://bio-database.net/graphs/occurrences> .
<{occurrence_uri}> <http://purl.org/dc/terms/created> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{occurrence_uri}> <http://purl.org/dc/terms/modified> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{occurrence_uri}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
"#
        );
        store
            .insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.clone().into_bytes());

        let frontend_nquads = format!(
            r#"_:updated <http://rs.tdwg.org/dwc/terms/scientificName> "Updated name" <https://bio-database.net/graphs/occurrences> .
_:updated <http://rs.tdwg.org/ac/terms/associatedMedia> <{media_uri}> <https://bio-database.net/graphs/occurrences> .
"#
        );
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/occurrences/{occurrence_id}"))
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={occurrence_owner_session}"))
                    .body(Body::from(frontend_nquads))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let stored_nquads = store
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("fake store should return occurrence")
            .expect("occurrence should still exist");
        assert_eq!(
            stored_nquads,
            existing_nquads.as_bytes(),
            "another user's media URI must not replace the existing occurrence RDF"
        );

        sqlx::query("DELETE FROM media_objects WHERE id = $1")
            .bind(media_id)
            .execute(&db)
            .await
            .expect("media metadata should be removed after test");
        sqlx::query("DELETE FROM sessions WHERE user_id IN ($1, $2)")
            .bind(occurrence_owner_id)
            .bind(media_owner_id)
            .execute(&db)
            .await
            .expect("test sessions should be removed");
        sqlx::query("DELETE FROM users WHERE id IN ($1, $2)")
            .bind(occurrence_owner_id)
            .bind(media_owner_id)
            .execute(&db)
            .await
            .expect("test users should be removed");
    }

    #[tokio::test]
    async fn update_occurrence_route_with_valid_session_updates_existing_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-update-user-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-update-user";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let created = "2026-06-02T10:20:30Z";

        let existing_nquads = format!(
            r#"<{}> <http://rs.tdwg.org/dwc/terms/scientificName> "Old name" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/created> "{}"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/modified> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri,
            occurrence_uri,
            user_id,
            occurrence_uri,
            created,
            occurrence_uri,
            occurrence_uri,
        );

        store.insert_occurrence_nquads(occurrence_uri.clone(), existing_nquads.into_bytes());

        let frontend_nquads = br#"_:updated <http://rs.tdwg.org/dwc/terms/scientificName> "Updated name" <https://bio-database.net/graphs/occurrences> .
_:updated <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#;

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(CONTENT_TYPE, "application/n-quads")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(frontend_nquads.to_vec()))
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
            content_type.starts_with("application/json"),
            "PUT /occurrences/{{occurrence_id}} should return JSON"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["occurrence_id"], occurrence_id.to_string());
        assert_eq!(body_json["occurrence_uri"], occurrence_uri);

        let updated_nquads = store
            .get_occurrence_nquads(&occurrence_uri)
            .await
            .expect("fake store should return updated occurrence")
            .expect("updated occurrence should exist");

        let parsed_quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(&updated_nquads)
            .collect::<Result<Vec<_>, _>>()
            .expect("updated N-Quads should parse");

        assert!(
            parsed_quads
                .iter()
                .all(|quad| { quad.subject.to_string() == format!("<{}>", occurrence_uri) })
        );
        assert!(parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://rs.tdwg.org/dwc/terms/scientificName>"
                && quad.object.to_string() == "\"Updated name\""
        }));
        assert!(parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/creator>"
                && quad.object.to_string()
                    == format!("<https://bio-database.net/users/{}>", user_id)
        }));
        assert!(parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/created>"
                && quad.object.to_string()
                    == format!(
                        "\"{}\"^^<http://www.w3.org/2001/XMLSchema#dateTime>",
                        created
                    )
        }));
        assert!(parsed_quads.iter().any(|quad| {
            quad.predicate.to_string() == "<http://purl.org/dc/terms/accessRights>"
                && quad.object.to_string()
                    == "<https://bio-database.net/terms/access-rights/public>"
        }));
    }

    #[tokio::test]
    async fn get_occurrence_route_returns_nquads_for_existing_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        let expected_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/test-user> <https://bio-database.net/graphs/occurrences> .
    "#,
            occurrence_uri, occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), expected_nquads.clone().into_bytes());

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            body.as_ref(),
            expected_nquads.as_bytes(),
            "response body should be occurrence N-Quads returned from OccurrenceRdfStore"
        );
    }

    #[tokio::test]
    async fn get_occurrence_route_allows_anonymous_user_to_view_public_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        let expected_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
    "#,
            occurrence_uri, occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), expected_nquads.clone().into_bytes());

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

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
            "public occurrence detail should be returned as N-Quads"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            body.as_ref(),
            expected_nquads.as_bytes(),
            "anonymous user should receive public occurrence N-Quads"
        );
    }

    #[tokio::test]
    async fn get_occurrence_route_hides_private_occurrence_from_anonymous_user() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        let private_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
    "#,
            occurrence_uri, occurrence_uri,
        );

        store.insert_occurrence_nquads(occurrence_uri, private_nquads.into_bytes());

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

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

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "occurrence_not_found");
        assert_eq!(body_json["message"], "Occurrence not found");
    }

    #[tokio::test]
    async fn get_occurrence_route_allows_editor_to_view_own_private_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-own-private-viewer-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-own-private-viewer";
        let password_hash = hash_password("password123").expect("password should be hashed");

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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        let expected_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
    "#,
            occurrence_uri, occurrence_uri, user_id, occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), expected_nquads.clone().into_bytes());

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
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
            "own private occurrence detail should be returned as N-Quads"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            body.as_ref(),
            expected_nquads.as_bytes(),
            "editor should receive own private occurrence N-Quads"
        );
    }

    #[tokio::test]
    async fn get_occurrence_route_hides_other_users_private_occurrence_from_editor() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-other-private-viewer-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-other-private-viewer";
        let password_hash = hash_password("password123").expect("password should be hashed");

        let viewer_user_id = sqlx::query_scalar!(
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
        .expect("viewer user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            viewer_user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let creator_user_id = uuid::Uuid::new_v4();
        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        let private_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
    "#,
            occurrence_uri, occurrence_uri, creator_user_id, occurrence_uri,
        );

        store.insert_occurrence_nquads(occurrence_uri, private_nquads.into_bytes());

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "occurrence_not_found");
        assert_eq!(body_json["message"], "Occurrence not found");
    }

    #[tokio::test]
    #[ignore]
    async fn get_occurrence_route_allows_admin_to_view_other_users_private_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let db = state.posgre.clone();

        let email = format!(
            "occurrence-admin-private-viewer-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "admin";
        let password_hash = hash_password("password123").expect("password should be hashed");

        let admin_user_id = sqlx::query_scalar!(
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
        .expect("admin user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            admin_user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let creator_user_id = uuid::Uuid::new_v4();
        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        let expected_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "Lumbricus terrestris" <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/creator> <https://bio-database.net/users/{}> <https://bio-database.net/graphs/occurrences> .
    <{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/private> <https://bio-database.net/graphs/occurrences> .
    "#,
            occurrence_uri, occurrence_uri, creator_user_id, occurrence_uri,
        );

        store
            .insert_occurrence_nquads(occurrence_uri.clone(), expected_nquads.clone().into_bytes());

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("/occurrences/{}", occurrence_id))
                    .header(COOKIE, format!("session={}", session_token))
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
            "admin should receive private occurrence detail as N-Quads"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            body.as_ref(),
            expected_nquads.as_bytes(),
            "admin should receive other user's private occurrence N-Quads"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn get_occurrence_route_returns_nquads_from_real_fuseki() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration test");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let scientific_name = format!("Real Fuseki detail {}", uuid::Uuid::new_v4());

        let stored_nquads = format!(
            r#"<{}> <https://example.org/vocab/scientificName> "{}" <https://bio-database.net/graphs/occurrences> .
<{}> <http://purl.org/dc/terms/accessRights> <https://bio-database.net/terms/access-rights/public> <https://bio-database.net/graphs/occurrences> .
"#,
            occurrence_uri, scientific_name, occurrence_uri,
        );

        fuseki_client
            .save_nquads(stored_nquads.into_bytes())
            .await
            .expect("test occurrence should be saved to real Fuseki");

        let state = AppState::new(config, posgre, Arc::new(fuseki_client));

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
            "real Fuseki occurrence detail should be returned as N-Quads"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let parsed_quads = RdfParser::from_format(RdfFormat::NQuads)
            .for_slice(body.as_ref())
            .collect::<Result<Vec<_>, _>>()
            .expect("response body should be valid N-Quads");

        let expected_subject = format!("<{}>", occurrence_uri);

        assert!(
            parsed_quads.iter().any(|quad| {
                quad.subject.to_string() == expected_subject
                    && quad.predicate.to_string() == "<https://example.org/vocab/scientificName>"
                    && quad.object.to_string() == format!("\"{}\"", scientific_name)
                    && quad.graph_name.to_string()
                        == "<https://bio-database.net/graphs/occurrences>"
            }),
            "GET /occurrences/{{occurrence_id}} should return scientificName from real Fuseki"
        );

        assert!(
            parsed_quads.iter().any(|quad| {
                quad.subject.to_string() == expected_subject
                    && quad.predicate.to_string() == "<http://purl.org/dc/terms/accessRights>"
                    && quad.object.to_string()
                        == "<https://bio-database.net/terms/access-rights/public>"
                    && quad.graph_name.to_string()
                        == "<https://bio-database.net/graphs/occurrences>"
            }),
            "GET /occurrences/{{occurrence_id}} should return public accessRights from real Fuseki"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn search_occurrences_route_returns_results_from_real_fuseki() {
        dotenvy::dotenv().ok();

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration test");

        let config = Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig {
                url: database_url.clone(),
            },
            garage: GarageConfig {
                bucket: std::env::var("S3_BUCKET")
                    .unwrap_or_else(|_| "occurrence-media".to_string()),
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

        let occurrence_id = uuid::Uuid::new_v4();
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);
        let graph_uri = "https://bio-database.net/graphs/occurrences";
        let scientific_name_predicate = "http://rs.tdwg.org/dwc/terms/scientificName";
        let created_predicate = "http://purl.org/dc/terms/created";
        let modified_predicate = "http://purl.org/dc/terms/modified";
        let access_rights_predicate = "http://purl.org/dc/terms/accessRights";
        let public_access_rights_uri = "https://bio-database.net/terms/access-rights/public";
        let scientific_name = format!("Real app search target {}", uuid::Uuid::new_v4());

        let nquads = format!(
            r#"<{}> <{}> "{}" <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> "2026-06-02T10:20:30Z"^^<http://www.w3.org/2001/XMLSchema#dateTime> <{}> .
<{}> <{}> <{}> <{}> .
"#,
            occurrence_uri,
            scientific_name_predicate,
            scientific_name,
            graph_uri,
            occurrence_uri,
            created_predicate,
            graph_uri,
            occurrence_uri,
            modified_predicate,
            graph_uri,
            occurrence_uri,
            access_rights_predicate,
            public_access_rights_uri,
            graph_uri,
        );

        fuseki_client
            .save_nquads(nquads.into_bytes())
            .await
            .expect("test occurrence should be saved to real Fuseki");

        let state = AppState::new(config, posgre, Arc::new(fuseki_client));
        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{
                            "filters":[{{
                                "predicate":"{}",
                                "value":"{}",
                                "value_type":"literal",
                                "match":"exact"
                            }}],
                            "page":{{"limit":50,"cursor":null}}
                        }}"#,
                        scientific_name_predicate, scientific_name
                    )))
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
            content_type.starts_with("application/json"),
            "search occurrence response should be JSON"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            body_json["items"][0]["occurrence_id"],
            occurrence_id.to_string()
        );
        assert_eq!(body_json["items"][0]["occurrence_uri"], occurrence_uri);
        assert_eq!(body_json["items"][0]["scientific_name"], scientific_name);
        assert_eq!(body_json["items"][0]["created"], "2026-06-02T10:20:30Z");
        assert_eq!(body_json["items"][0]["modified"], "2026-06-02T10:20:30Z");
        assert_eq!(body_json["items"][0]["access_rights"], "public");
        assert_eq!(body_json["page"]["limit"], 50);
        assert_eq!(body_json["page"]["next_cursor"], serde_json::Value::Null);
        assert_eq!(body_json["page"]["has_next"], false);
    }

    #[tokio::test]
    async fn search_occurrences_route_returns_store_results_for_empty_search() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![SearchOccurrenceStoreRow {
                occurrence_id,
                occurrence_uri: occurrence_uri.clone(),
                creator_user_id: None,
                scientific_name: Some("Quercus serrata".to_string()),
                basis_of_record: Some("PreservedSpecimen".to_string()),
                recorded_by: Some("Yamada Taro".to_string()),
                created: Some("2026-06-02T10:20:30Z".to_string()),
                modified: Some("2026-06-02T10:20:30Z".to_string()),
                access_rights: Some("public".to_string()),
            }],
            limit: 50,
            next_cursor: Some("opaque-cursor-string".to_string()),
            has_next: true,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"filters":[],"page":{"limit":50,"cursor":null}}"#,
                    ))
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
            content_type.starts_with("application/json"),
            "search occurrence response should be JSON"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            body_json["items"][0]["occurrence_id"],
            occurrence_id.to_string()
        );
        assert_eq!(body_json["items"][0]["occurrence_uri"], occurrence_uri);
        assert_eq!(body_json["items"][0]["scientific_name"], "Quercus serrata");
        assert_eq!(
            body_json["items"][0]["basis_of_record"],
            "PreservedSpecimen"
        );
        assert_eq!(body_json["items"][0]["recorded_by"], "Yamada Taro");
        assert_eq!(body_json["items"][0]["created"], "2026-06-02T10:20:30Z");
        assert_eq!(body_json["items"][0]["modified"], "2026-06-02T10:20:30Z");
        assert_eq!(body_json["items"][0]["access_rights"], "public");

        assert_eq!(body_json["page"]["limit"], 50);
        assert_eq!(body_json["page"]["next_cursor"], "opaque-cursor-string");
        assert_eq!(body_json["page"]["has_next"], true);

        assert_eq!(store.requested_search_inputs(), vec![(50, None)]);
    }

    #[tokio::test]
    async fn search_occurrences_route_defaults_limit_to_50_when_omitted() {
        let store = FakeOccurrenceRdfStore::default();

        let occurrence_id =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let occurrence_uri = format!("https://bio-database.net/occurrences/{}", occurrence_id);

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![SearchOccurrenceStoreRow {
                occurrence_id,
                occurrence_uri: occurrence_uri.clone(),
                creator_user_id: None,
                scientific_name: Some("Quercus serrata".to_string()),
                basis_of_record: Some("PreservedSpecimen".to_string()),
                recorded_by: Some("Yamada Taro".to_string()),
                created: Some("2026-06-02T10:20:30Z".to_string()),
                modified: Some("2026-06-02T10:20:30Z".to_string()),
                access_rights: Some("public".to_string()),
            }],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"filters":[],"page":{"cursor":null}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["page"]["limit"], 50);
        assert_eq!(body_json["page"]["next_cursor"], serde_json::Value::Null);
        assert_eq!(body_json["page"]["has_next"], false);
        assert_eq!(body_json["items"].as_array().unwrap().len(), 1);
        assert_eq!(store.requested_search_inputs(), vec![(50, None)]);
    }

    #[tokio::test]
    async fn search_occurrences_route_applies_filter_to_store_results() {
        let store = FakeOccurrenceRdfStore::default();

        let matching_occurrence_id =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let matching_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            matching_occurrence_id
        );

        let other_occurrence_id =
            uuid::Uuid::parse_str("660e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let other_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            other_occurrence_id
        );

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![
                SearchOccurrenceStoreRow {
                    occurrence_id: matching_occurrence_id,
                    occurrence_uri: matching_occurrence_uri.clone(),
                    creator_user_id: None,
                    scientific_name: Some("Quercus serrata".to_string()),
                    basis_of_record: Some("PreservedSpecimen".to_string()),
                    recorded_by: Some("Yamada Taro".to_string()),
                    created: Some("2026-06-02T10:20:30Z".to_string()),
                    modified: Some("2026-06-02T10:20:30Z".to_string()),
                    access_rights: Some("public".to_string()),
                },
                SearchOccurrenceStoreRow {
                    occurrence_id: other_occurrence_id,
                    occurrence_uri: other_occurrence_uri,
                    creator_user_id: None,
                    scientific_name: Some("Acer palmatum".to_string()),
                    basis_of_record: Some("HumanObservation".to_string()),
                    recorded_by: Some("Suzuki Jiro".to_string()),
                    created: Some("2026-06-02T10:20:31Z".to_string()),
                    modified: Some("2026-06-02T10:20:31Z".to_string()),
                    access_rights: Some("public".to_string()),
                },
            ],
            limit: 50,
            next_cursor: Some("opaque-cursor-string".to_string()),
            has_next: true,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "filters":[{
                                "predicate":"http://rs.tdwg.org/dwc/terms/scientificName",
                                "value":"Quercus serrata",
                                "value_type":"literal",
                                "match":"exact"
                            }],
                            "page":{"limit":50,"cursor":null}
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            body_json["items"][0]["occurrence_id"],
            matching_occurrence_id.to_string()
        );
        assert_eq!(
            body_json["items"][0]["occurrence_uri"],
            matching_occurrence_uri
        );
        assert_eq!(body_json["items"][0]["scientific_name"], "Quercus serrata");
        assert_eq!(body_json["page"]["limit"], 50);
        assert_eq!(body_json["page"]["next_cursor"], serde_json::Value::Null);
        assert_eq!(body_json["page"]["has_next"], false);
    }

    #[tokio::test]
    async fn search_occurrences_route_matches_literal_filter_case_insensitively() {
        let store = FakeOccurrenceRdfStore::default();

        let matching_occurrence_id =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let matching_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            matching_occurrence_id
        );

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![SearchOccurrenceStoreRow {
                occurrence_id: matching_occurrence_id,
                occurrence_uri: matching_occurrence_uri.clone(),
                creator_user_id: None,
                scientific_name: Some("Quercus serrata".to_string()),
                basis_of_record: Some("PreservedSpecimen".to_string()),
                recorded_by: Some("Yamada Taro".to_string()),
                created: Some("2026-06-02T10:20:30Z".to_string()),
                modified: Some("2026-06-02T10:20:30Z".to_string()),
                access_rights: Some("public".to_string()),
            }],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "filters":[{
                                "predicate":"http://rs.tdwg.org/dwc/terms/scientificName",
                                "value":"quercus serrata",
                                "value_type":"literal",
                                "match":"exact"
                            }],
                            "page":{"limit":50,"cursor":null}
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            body_json["items"][0]["occurrence_id"],
            matching_occurrence_id.to_string()
        );
        assert_eq!(body_json["items"][0]["scientific_name"], "Quercus serrata");
    }

    #[tokio::test]
    async fn search_occurrences_route_trims_literal_filter_value() {
        let store = FakeOccurrenceRdfStore::default();

        let matching_occurrence_id =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let matching_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            matching_occurrence_id
        );

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![SearchOccurrenceStoreRow {
                occurrence_id: matching_occurrence_id,
                occurrence_uri: matching_occurrence_uri.clone(),
                creator_user_id: None,
                scientific_name: Some("Quercus serrata".to_string()),
                basis_of_record: Some("PreservedSpecimen".to_string()),
                recorded_by: Some("Yamada Taro".to_string()),
                created: Some("2026-06-02T10:20:30Z".to_string()),
                modified: Some("2026-06-02T10:20:30Z".to_string()),
                access_rights: Some("public".to_string()),
            }],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "filters":[{
                                "predicate":"http://rs.tdwg.org/dwc/terms/scientificName",
                                "value":"  Quercus serrata  ",
                                "value_type":"literal",
                                "match":"exact"
                            }],
                            "page":{"limit":50,"cursor":null}
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            body_json["items"][0]["occurrence_id"],
            matching_occurrence_id.to_string()
        );
        assert_eq!(body_json["items"][0]["scientific_name"], "Quercus serrata");
    }

    #[tokio::test]
    async fn search_occurrences_route_rejects_invalid_filter_value_type() {
        let store = FakeOccurrenceRdfStore::default();

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "filters":[{
                                "predicate":"http://rs.tdwg.org/dwc/terms/scientificName",
                                "value":"Quercus serrata",
                                "value_type":"number",
                                "match":"exact"
                            }],
                            "page":{"limit":50,"cursor":null}
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            store.requested_search_inputs().is_empty(),
            "invalid filter value_type should be rejected before searching OccurrenceRdfStore"
        );
    }

    #[tokio::test]
    async fn search_occurrences_route_rejects_invalid_filter_match() {
        let store = FakeOccurrenceRdfStore::default();

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "filters":[{
                                "predicate":"http://rs.tdwg.org/dwc/terms/scientificName",
                                "value":"Quercus",
                                "value_type":"literal",
                                "match":"contains"
                            }],
                            "page":{"limit":50,"cursor":null}
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            store.requested_search_inputs().is_empty(),
            "invalid filter match should be rejected before searching OccurrenceRdfStore"
        );
    }

    #[tokio::test]
    async fn search_occurrences_route_rejects_non_absolute_filter_predicate() {
        let store = FakeOccurrenceRdfStore::default();

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "filters":[{
                                "predicate":"dwc:scientificName",
                                "value":"Quercus serrata",
                                "value_type":"literal",
                                "match":"exact"
                            }],
                            "page":{"limit":50,"cursor":null}
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(
            store.requested_search_inputs().is_empty(),
            "non-absolute filter predicate should be rejected before searching OccurrenceRdfStore"
        );
    }

    #[tokio::test]
    async fn search_occurrences_route_hides_private_occurrences_from_anonymous_user() {
        let store = FakeOccurrenceRdfStore::default();

        let public_occurrence_id =
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let public_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            public_occurrence_id
        );

        let private_occurrence_id =
            uuid::Uuid::parse_str("660e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let private_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            private_occurrence_id
        );

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![
                SearchOccurrenceStoreRow {
                    occurrence_id: public_occurrence_id,
                    occurrence_uri: public_occurrence_uri.clone(),
                    creator_user_id: None,
                    scientific_name: Some("Quercus serrata".to_string()),
                    basis_of_record: Some("PreservedSpecimen".to_string()),
                    recorded_by: Some("Yamada Taro".to_string()),
                    created: Some("2026-06-02T10:20:30Z".to_string()),
                    modified: Some("2026-06-02T10:20:30Z".to_string()),
                    access_rights: Some("public".to_string()),
                },
                SearchOccurrenceStoreRow {
                    occurrence_id: private_occurrence_id,
                    occurrence_uri: private_occurrence_uri,
                    creator_user_id: None,
                    scientific_name: Some("Acer palmatum".to_string()),
                    basis_of_record: Some("HumanObservation".to_string()),
                    recorded_by: Some("Suzuki Jiro".to_string()),
                    created: Some("2026-06-02T10:20:31Z".to_string()),
                    modified: Some("2026-06-02T10:20:31Z".to_string()),
                    access_rights: Some("private".to_string()),
                },
            ],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"filters":[],"page":{"limit":50,"cursor":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            body_json["items"][0]["occurrence_id"],
            public_occurrence_id.to_string()
        );
        assert_eq!(
            body_json["items"][0]["occurrence_uri"],
            public_occurrence_uri
        );
        assert_eq!(body_json["items"][0]["access_rights"], "public");
    }

    #[tokio::test]
    async fn search_occurrences_route_returns_empty_page_when_only_private_results_are_available_to_anonymous_user()
     {
        let store = FakeOccurrenceRdfStore::default();

        let first_private_occurrence_id =
            uuid::Uuid::parse_str("660e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let first_private_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            first_private_occurrence_id
        );
        let second_private_occurrence_id =
            uuid::Uuid::parse_str("770e8400-e29b-41d4-a716-446655440000").expect("valid uuid");
        let second_private_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            second_private_occurrence_id
        );

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![
                SearchOccurrenceStoreRow {
                    occurrence_id: first_private_occurrence_id,
                    occurrence_uri: first_private_occurrence_uri,
                    creator_user_id: None,
                    scientific_name: Some("Acer palmatum".to_string()),
                    basis_of_record: Some("HumanObservation".to_string()),
                    recorded_by: Some("Suzuki Jiro".to_string()),
                    created: Some("2026-06-02T10:20:31Z".to_string()),
                    modified: Some("2026-06-02T10:20:31Z".to_string()),
                    access_rights: Some("private".to_string()),
                },
                SearchOccurrenceStoreRow {
                    occurrence_id: second_private_occurrence_id,
                    occurrence_uri: second_private_occurrence_uri,
                    creator_user_id: None,
                    scientific_name: Some("Acer japonicum".to_string()),
                    basis_of_record: Some("HumanObservation".to_string()),
                    recorded_by: Some("Suzuki Jiro".to_string()),
                    created: Some("2026-06-02T10:20:30Z".to_string()),
                    modified: Some("2026-06-02T10:20:30Z".to_string()),
                    access_rights: Some("private".to_string()),
                },
            ],
            limit: 1,
            next_cursor: Some("cursor-after-private-row".to_string()),
            has_next: true,
        });

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"filters":[],"page":{"limit":1,"cursor":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body_json["items"].as_array().unwrap().len(), 0);
        assert_eq!(body_json["page"]["limit"], 1);
        assert_eq!(body_json["page"]["next_cursor"], serde_json::Value::Null);
        assert_eq!(body_json["page"]["has_next"], false);
    }

    #[tokio::test]
    async fn search_occurrences_route_allows_editor_to_view_own_private_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-search-own-private-viewer-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-search-own-private-viewer";
        let password_hash = hash_password("password123").expect("password should be hashed");

        let viewer_user_id = sqlx::query_scalar!(
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
        .expect("viewer user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            viewer_user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let own_private_occurrence_id = uuid::Uuid::new_v4();
        let own_private_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            own_private_occurrence_id
        );

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![SearchOccurrenceStoreRow {
                occurrence_id: own_private_occurrence_id,
                occurrence_uri: own_private_occurrence_uri.clone(),
                creator_user_id: Some(viewer_user_id),
                scientific_name: Some("Acer palmatum".to_string()),
                basis_of_record: Some("HumanObservation".to_string()),
                recorded_by: Some("Suzuki Jiro".to_string()),
                created: Some("2026-06-02T10:20:31Z".to_string()),
                modified: Some("2026-06-02T10:20:31Z".to_string()),
                access_rights: Some("private".to_string()),
            }],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(
                        r#"{"filters":[],"page":{"limit":50,"cursor":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let items = body_json["items"]
            .as_array()
            .expect("items should be array");

        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0]["occurrence_id"],
            own_private_occurrence_id.to_string()
        );
        assert_eq!(items[0]["occurrence_uri"], own_private_occurrence_uri);
        assert_eq!(items[0]["access_rights"], "private");
    }

    #[tokio::test]
    async fn search_occurrences_route_hides_other_users_private_occurrences_from_editor() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));
        let db = state.posgre.clone();

        let email = format!(
            "occurrence-search-private-viewer-{}@example.com",
            uuid::Uuid::new_v4()
        );
        let user_name = "occurrence-search-private-viewer";
        let password_hash = hash_password("password123").expect("password should be hashed");

        let viewer_user_id = sqlx::query_scalar!(
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
        .expect("viewer user should be inserted");

        let session_token = uuid::Uuid::new_v4().to_string();
        let session_token_hash = hash_token(&session_token);

        sqlx::query!(
            r#"
            INSERT INTO sessions (user_id, session_token_hash, expires_at)
            VALUES ($1, $2, now() + interval '30 days')
            "#,
            viewer_user_id,
            session_token_hash
        )
        .execute(&db)
        .await
        .expect("session should be inserted");

        let public_occurrence_id = uuid::Uuid::new_v4();
        let public_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            public_occurrence_id
        );
        let own_private_occurrence_id = uuid::Uuid::new_v4();
        let own_private_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            own_private_occurrence_id
        );
        let other_private_occurrence_id = uuid::Uuid::new_v4();
        let other_private_occurrence_uri = format!(
            "https://bio-database.net/occurrences/{}",
            other_private_occurrence_id
        );
        let other_creator_user_id = uuid::Uuid::new_v4();

        store.set_search_page(SearchOccurrencesStorePage {
            rows: vec![
                SearchOccurrenceStoreRow {
                    occurrence_id: public_occurrence_id,
                    occurrence_uri: public_occurrence_uri.clone(),
                    creator_user_id: Some(other_creator_user_id),
                    scientific_name: Some("Quercus serrata".to_string()),
                    basis_of_record: Some("PreservedSpecimen".to_string()),
                    recorded_by: Some("Yamada Taro".to_string()),
                    created: Some("2026-06-02T10:20:30Z".to_string()),
                    modified: Some("2026-06-02T10:20:30Z".to_string()),
                    access_rights: Some("public".to_string()),
                },
                SearchOccurrenceStoreRow {
                    occurrence_id: own_private_occurrence_id,
                    occurrence_uri: own_private_occurrence_uri.clone(),
                    creator_user_id: Some(viewer_user_id),
                    scientific_name: Some("Acer palmatum".to_string()),
                    basis_of_record: Some("HumanObservation".to_string()),
                    recorded_by: Some("Suzuki Jiro".to_string()),
                    created: Some("2026-06-02T10:20:31Z".to_string()),
                    modified: Some("2026-06-02T10:20:31Z".to_string()),
                    access_rights: Some("private".to_string()),
                },
                SearchOccurrenceStoreRow {
                    occurrence_id: other_private_occurrence_id,
                    occurrence_uri: other_private_occurrence_uri,
                    creator_user_id: Some(other_creator_user_id),
                    scientific_name: Some("Pinus densiflora".to_string()),
                    basis_of_record: Some("HumanObservation".to_string()),
                    recorded_by: Some("Sato Saburo".to_string()),
                    created: Some("2026-06-02T10:20:32Z".to_string()),
                    modified: Some("2026-06-02T10:20:32Z".to_string()),
                    access_rights: Some("private".to_string()),
                },
            ],
            limit: 50,
            next_cursor: None,
            has_next: false,
        });

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/occurrences/search")
                    .header(CONTENT_TYPE, "application/json")
                    .header(COOKIE, format!("session={}", session_token))
                    .body(Body::from(
                        r#"{"filters":[],"page":{"limit":50,"cursor":null}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let items = body_json["items"]
            .as_array()
            .expect("items should be array");

        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .any(|item| item["occurrence_id"] == public_occurrence_id.to_string())
        );
        assert!(
            items
                .iter()
                .any(|item| item["occurrence_id"] == own_private_occurrence_id.to_string())
        );
        assert!(
            !items
                .iter()
                .any(|item| item["occurrence_id"] == other_private_occurrence_id.to_string())
        );
    }

    #[tokio::test]
    async fn get_occurrence_route_returns_not_found_for_missing_occurrence() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "occurrence_not_found");
        assert_eq!(body_json["message"], "Occurrence not found");
    }

    #[tokio::test]
    async fn get_occurrence_route_returns_bad_request_for_invalid_occurrence_id() {
        let store = FakeOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

        let app = build_app(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/occurrences/not-a-uuid")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_occurrence_route_when_rdf_store_fails_returns_bad_gateway() {
        let store = FailingOccurrenceRdfStore::default();

        let state = test_state_with_occurrence_rdf_store(Arc::new(store.clone()));

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

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        let body_json: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be JSON");

        assert_eq!(body_json["error"], "rdf_store_error");
        assert_eq!(body_json["message"], "Failed to save occurrence RDF");
    }
}
