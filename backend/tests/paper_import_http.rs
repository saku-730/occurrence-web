use std::{
    collections::VecDeque,
    ffi::OsString,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::State,
    http::{
        HeaderMap, Request, StatusCode,
        header::{ACCEPT, CONTENT_TYPE, COOKIE},
    },
    response::{IntoResponse, Response},
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

const BIBTEX: &str = r#"@article{sample,
  author = {Doe, Jane and Smith, John Q.},
  title = {A study of earthworms},
  journal = {Example Journal},
  year = {2025},
  volume = {12},
  number = {3},
  pages = {101--115},
  doi = {10.1234/example.1}
}"#;

#[derive(Clone)]
struct MockGrobidState {
    responses: Arc<Mutex<VecDeque<(StatusCode, String)>>>,
    request_count: Arc<Mutex<usize>>,
}

async fn mock_grobid_handler(
    State(state): State<MockGrobidState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    assert_eq!(
        headers.get(ACCEPT).and_then(|value| value.to_str().ok()),
        Some("application/x-bibtex")
    );
    assert!(
        headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
    );
    assert!(String::from_utf8_lossy(&body).contains("%PDF-"));

    *state
        .request_count
        .lock()
        .expect("request count lock poisoned") += 1;
    let (status, response_body) = state
        .responses
        .lock()
        .expect("response lock poisoned")
        .pop_front()
        .unwrap_or((StatusCode::OK, BIBTEX.to_string()));
    (status, response_body).into_response()
}

async fn start_mock_grobid(
    responses: Vec<(StatusCode, String)>,
) -> (String, Arc<Mutex<usize>>, tokio::task::JoinHandle<()>) {
    let state = MockGrobidState {
        responses: Arc::new(Mutex::new(responses.into())),
        request_count: Arc::new(Mutex::new(0)),
    };
    let request_count = state.request_count.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock GROBID");
    let address = listener.local_addr().expect("mock GROBID address");
    let app = Router::new()
        .route("/api/processHeaderDocument", post(mock_grobid_handler))
        .with_state(state);
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock GROBID server failed");
    });
    (format!("http://{address}"), request_count, handle)
}

struct EnvGuard {
    key: &'static str,
    old_value: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old_value = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
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

#[derive(Clone, Default)]
struct RecordingObjectStore {
    puts: Arc<Mutex<usize>>,
    deletes: Arc<Mutex<usize>>,
}

impl RecordingObjectStore {
    fn put_count(&self) -> usize {
        *self.puts.lock().expect("puts lock poisoned")
    }
}

#[async_trait::async_trait]
impl MediaObjectStore for RecordingObjectStore {
    async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
        let bytes = tokio::fs::read(&input.file_path)
            .await
            .map_err(MediaServiceError::FileSystem)?;
        assert!(bytes.starts_with(b"%PDF-"));
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
        *self.deletes.lock().expect("deletes lock poisoned") += 1;
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

fn database_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in the environment or .env for paper import tests")
}

async fn test_db_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url())
        .await
        .expect("failed to connect test PostgreSQL")
}

async fn create_test_user_and_session(db: &PgPool) -> (Uuid, String) {
    let user_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO users (id, email, user_name, password_hash)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(user_id)
    .bind(format!("paper-http-{user_id}@example.com"))
    .bind(format!("paper-http-{user_id}"))
    .bind("test-password-hash")
    .execute(db)
    .await
    .expect("failed to create test user");

    let session_token = Uuid::new_v4().to_string();
    let session_token_hash = hash_token(&session_token);
    sqlx::query(
        r#"
        INSERT INTO sessions (user_id, session_token_hash, expires_at)
        VALUES ($1, $2, now() + interval '1 day')
        "#,
    )
    .bind(user_id)
    .bind(session_token_hash)
    .execute(db)
    .await
    .expect("failed to create test session");

    (user_id, session_token)
}

fn test_state(db: PgPool, store: RecordingObjectStore) -> AppState {
    let config = Config {
        app: AppConfig {
            host: "127.0.0.1".to_string(),
            port: 3000,
            app_base_url: "http://127.0.0.1:3000".to_string(),
            environment: "test".to_string(),
            cookie_secure: false,
        },
        posgre: PosgreConfig { url: database_url() },
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
    };

    AppState::new_with_media_object_store(
        config,
        db,
        Arc::new(NoopOccurrenceRdfStore),
        Arc::new(store),
    )
}

fn multipart_body(
    boundary: &str,
    filename: &str,
    declared_content_type: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {declared_content_type}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
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

#[tokio::test(flavor = "current_thread")]
async fn authenticated_pdf_request_returns_created_and_persists_grobid_metadata() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, session_token) = create_test_user_and_session(&db).await;
    let (grobid_url, request_count, server) =
        start_mock_grobid(vec![(StatusCode::OK, BIBTEX.to_string())]).await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));

    let boundary = "paper-import-http-test-boundary";
    let pdf = b"%PDF-1.7\nvalid test paper\n";
    let body = multipart_body(boundary, "paper.pdf", "application/pdf", pdf);
    let request = Request::builder()
        .method("POST")
        .uri("/paper-import")
        .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .header(COOKIE, format!("session={session_token}"))
        .body(Body::from(body))
        .expect("failed to build request");

    let response = app.oneshot(request).await.expect("paper import response");
    assert_eq!(response.status(), StatusCode::CREATED);
    let response_body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&response_body).expect("response should be JSON");
    assert_eq!(json["status"], "imported");
    assert_eq!(json["doi"], "10.1234/example.1");
    assert_eq!(json["title"], "A study of earthworms");
    assert_eq!(json["publication_year"], 2025);
    assert_eq!(store.put_count(), 1);
    assert_eq!(*request_count.lock().expect("request count lock poisoned"), 1);

    let paper_id = Uuid::parse_str(json["paper_id"].as_str().expect("paper_id string"))
        .expect("paper_id UUID");
    let row: (Option<String>, Option<String>, Option<i32>) = sqlx::query_as(
        "SELECT doi, title, publication_year FROM papers WHERE id = $1",
    )
    .bind(paper_id)
    .fetch_one(&db)
    .await
    .expect("failed to load saved paper");
    assert_eq!(row.0.as_deref(), Some("10.1234/example.1"));
    assert_eq!(row.1.as_deref(), Some("A study of earthworms"));
    assert_eq!(row.2, Some(2025));

    cleanup_user(&db, user_id).await;
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn fake_pdf_is_rejected_before_garage_or_grobid() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, session_token) = create_test_user_and_session(&db).await;
    let (grobid_url, request_count, server) =
        start_mock_grobid(vec![(StatusCode::OK, BIBTEX.to_string())]).await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));

    let boundary = "paper-import-http-fake-pdf-boundary";
    let body = multipart_body(
        boundary,
        "fake.pdf",
        "application/pdf",
        b"this is not a PDF",
    );
    let request = Request::builder()
        .method("POST")
        .uri("/paper-import")
        .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
        .header(COOKIE, format!("session={session_token}"))
        .body(Body::from(body))
        .expect("failed to build request");

    let response = app.oneshot(request).await.expect("paper import response");
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(store.put_count(), 0, "fake PDF must not reach Garage");
    assert_eq!(
        *request_count.lock().expect("request count lock poisoned"),
        0,
        "fake PDF must not reach GROBID"
    );

    cleanup_user(&db, user_id).await;
    server.abort();
}
