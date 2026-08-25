use std::{
    ffi::OsString,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use axum::{
    Router,
    body::Body,
    http::{
        Request, StatusCode,
        header::{CONTENT_LENGTH, CONTENT_TYPE, COOKIE},
    },
    routing::post,
};
use backend::{
    config::{AppConfig, Config, FusekiConfig, GarageConfig, PosgreConfig, SmtpConfig},
    features::{
        auth::service::hash_token,
        media::service::{
            DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectByteStream, MediaObjectStore,
            MediaServiceError, PutMediaObjectInput,
        },
        occurrences::service::{DarwinCoreTerm, OccurrenceRdfStore, OccurrenceServiceError},
        paper_import,
    },
    state::AppState,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

struct EnvGuard {
    key: &'static str,
    old_value: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old_value = std::env::var_os(key);
        unsafe { std::env::set_var(key, value); }
        Self { key, old_value }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.old_value {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("environment lock poisoned")
}

async fn start_counting_grobid() -> (String, Arc<Mutex<usize>>, tokio::task::JoinHandle<()>) {
    let count = Arc::new(Mutex::new(0_usize));
    let handler_count = count.clone();
    let app = Router::new().route(
        "/api/processHeaderDocument",
        post(move || {
            let handler_count = handler_count.clone();
            async move {
                *handler_count.lock().expect("GROBID count lock poisoned") += 1;
                "@article{sample,title={Should not be called}}"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock GROBID");
    let address = listener.local_addr().expect("mock GROBID address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock GROBID failed");
    });
    (format!("http://{address}"), count, handle)
}

#[derive(Clone, Default)]
struct RecordingObjectStore {
    puts: Arc<Mutex<usize>>,
}

impl RecordingObjectStore {
    fn put_count(&self) -> usize {
        *self.puts.lock().expect("puts lock poisoned")
    }
}

#[async_trait::async_trait]
impl MediaObjectStore for RecordingObjectStore {
    async fn put_object(&self, _input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
        *self.puts.lock().expect("puts lock poisoned") += 1;
        Ok(())
    }

    async fn get_object(
        &self,
        _input: GetMediaObjectInput,
    ) -> Result<MediaObjectByteStream, MediaServiceError> {
        Err(MediaServiceError::ObjectStoreFailed)
    }

    async fn delete_object(&self, _input: DeleteMediaObjectInput) -> Result<(), MediaServiceError> {
        Ok(())
    }
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

    async fn list_darwin_core_terms(&self) -> Result<Vec<DarwinCoreTerm>, OccurrenceServiceError> {
        Ok(Vec::new())
    }
}

async fn test_db_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set for paper import tests");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect test PostgreSQL")
}

fn test_state(db: PgPool, store: RecordingObjectStore) -> AppState {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    AppState::new_with_media_object_store(
        Config {
            app: AppConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                app_base_url: "http://127.0.0.1:3000".to_string(),
                environment: "test".to_string(),
                cookie_secure: false,
            },
            posgre: PosgreConfig { url: database_url },
            smtp: SmtpConfig {
                host: "127.0.0.1".to_string(),
                port: 1025,
                username: String::new(),
                password: String::new(),
                tls: "none".to_string(),
                from: "no-reply@example.com".to_string(),
            },
            fuseki: FusekiConfig {
                base_url: "http://127.0.0.1:3030/occurrence".to_string(),
                user: "test".to_string(),
                password: "test".to_string(),
            },
            garage: GarageConfig {
                bucket: "occurrence-media".to_string(),
            },
        },
        db,
        Arc::new(NoopOccurrenceRdfStore),
        Arc::new(store),
    )
}

async fn create_test_user_and_session(db: &PgPool) -> (Uuid, String) {
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, user_name, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(format!("paper-error-{user_id}@example.com"))
    .bind(format!("paper-error-{user_id}"))
    .bind("test-password-hash")
    .execute(db)
    .await
    .expect("failed to create test user");

    let token = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO sessions (user_id, session_token_hash, expires_at) VALUES ($1, $2, now() + interval '1 day')",
    )
    .bind(user_id)
    .bind(hash_token(&token))
    .execute(db)
    .await
    .expect("failed to create test session");
    (user_id, token)
}

async fn cleanup_user(db: &PgPool, user_id: Uuid) {
    sqlx::query("DELETE FROM papers WHERE uploaded_by = $1")
        .bind(user_id)
        .execute(db)
        .await
        .expect("failed to delete test papers");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(db)
        .await
        .expect("failed to delete test user");
}

fn multipart_body(boundary: &str, filename: &str, mime: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[tokio::test(flavor = "current_thread")]
async fn missing_session_returns_401_without_side_effects() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (grobid_url, grobid_count, server) = start_counting_grobid().await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db, store.clone()));
    let boundary = "missing-session-boundary";
    let body = multipart_body(boundary, "paper.pdf", "application/pdf", b"%PDF-1.7\n");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/paper-import")
                .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body))
                .expect("failed to build request"),
        )
        .await
        .expect("request failed");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(store.put_count(), 0);
    assert_eq!(*grobid_count.lock().expect("GROBID count lock poisoned"), 0);
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn non_pdf_filename_is_rejected_before_side_effects() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, token) = create_test_user_and_session(&db).await;
    let (grobid_url, grobid_count, server) = start_counting_grobid().await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));
    let boundary = "wrong-extension-boundary";
    let body = multipart_body(boundary, "paper.txt", "application/pdf", b"%PDF-1.7\n");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/paper-import")
                .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .header(COOKIE, format!("session={token}"))
                .body(Body::from(body))
                .expect("failed to build request"),
        )
        .await
        .expect("request failed");

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(store.put_count(), 0);
    assert_eq!(*grobid_count.lock().expect("GROBID count lock poisoned"), 0);
    cleanup_user(&db, user_id).await;
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn non_pdf_declared_mime_is_rejected_before_side_effects() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, token) = create_test_user_and_session(&db).await;
    let (grobid_url, grobid_count, server) = start_counting_grobid().await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));
    let boundary = "wrong-mime-boundary";
    let body = multipart_body(boundary, "paper.pdf", "text/plain", b"%PDF-1.7\n");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/paper-import")
                .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .header(COOKIE, format!("session={token}"))
                .body(Body::from(body))
                .expect("failed to build request"),
        )
        .await
        .expect("request failed");

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(store.put_count(), 0);
    assert_eq!(*grobid_count.lock().expect("GROBID count lock poisoned"), 0);
    cleanup_user(&db, user_id).await;
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_content_length_returns_413_before_side_effects() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, token) = create_test_user_and_session(&db).await;
    let (grobid_url, grobid_count, server) = start_counting_grobid().await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));
    let boundary = "oversized-boundary";
    let body = multipart_body(boundary, "paper.pdf", "application/pdf", b"%PDF-1.7\n");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/paper-import")
                .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .header(COOKIE, format!("session={token}"))
                .header(
                    CONTENT_LENGTH,
                    (paper_import::handler::PAPER_PDF_REQUEST_BODY_LIMIT_BYTES as u64 + 1)
                        .to_string(),
                )
                .body(Body::from(body))
                .expect("failed to build request"),
        )
        .await
        .expect("request failed");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(store.put_count(), 0);
    assert_eq!(*grobid_count.lock().expect("GROBID count lock poisoned"), 0);
    cleanup_user(&db, user_id).await;
    server.abort();
}
