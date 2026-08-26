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
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    fn delete_count(&self) -> usize {
        *self.deletes.lock().expect("deletes lock poisoned")
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
        posgre: PosgreConfig {
            url: database_url(),
        },
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

async fn insert_test_paper(
    db: &PgPool,
    uploaded_by: Uuid,
    doi: Option<&str>,
    title: Option<&str>,
) -> Uuid {
    let paper_id = Uuid::new_v4();
    let sha256 = format!("{}{}", paper_id.simple(), paper_id.simple());
    sqlx::query(
        r#"
        INSERT INTO papers (
            id, bucket, object_key, content_type, size_bytes,
            original_filename, sha256, doi, title, uploaded_by
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(paper_id)
    .bind("occurrence-media")
    .bind(format!("papers/{paper_id}/original.pdf"))
    .bind("application/pdf")
    .bind(123_i64)
    .bind("bibliographic-http-test.pdf")
    .bind(sha256)
    .bind(doi)
    .bind(title)
    .bind(uploaded_by)
    .execute(db)
    .await
    .expect("failed to insert test paper");
    paper_id
}

fn bibliographic_patch_request(
    paper_id: &str,
    token: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut request = Request::builder()
        .method("PATCH")
        .uri(format!("/papers/{paper_id}/bibliographic-metadata"))
        .header(CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(COOKIE, format!("session={token}"));
    }
    request
        .body(Body::from(body.to_string()))
        .expect("failed to build bibliographic PATCH request")
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
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
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
    assert_eq!(
        *request_count.lock().expect("request count lock poisoned"),
        1
    );

    let paper_id = Uuid::parse_str(json["paper_id"].as_str().expect("paper_id string"))
        .expect("paper_id UUID");
    let row: (Option<String>, Option<String>, Option<i32>) =
        sqlx::query_as("SELECT doi, title, publication_year FROM papers WHERE id = $1")
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
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
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

async fn send_pdf_request(
    app: Router,
    session_token: &str,
    boundary: &str,
    filename: &str,
    pdf: &[u8],
) -> Response {
    let body = multipart_body(boundary, filename, "application/pdf", pdf);
    let request = Request::builder()
        .method("POST")
        .uri("/paper-import")
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header(COOKIE, format!("session={session_token}"))
        .body(Body::from(body))
        .expect("failed to build request");

    app.oneshot(request).await.expect("paper import response")
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_pdf_request_returns_ok_without_repeating_side_effects() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, session_token) = create_test_user_and_session(&db).await;
    let (grobid_url, request_count, server) =
        start_mock_grobid(vec![(StatusCode::OK, BIBTEX.to_string())]).await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));
    let pdf = b"%PDF-1.7\nHTTP duplicate paper\n";

    let first = send_pdf_request(
        app.clone(),
        &session_token,
        "http-duplicate-first",
        "paper.pdf",
        pdf,
    )
    .await;
    let second = send_pdf_request(
        app,
        &session_token,
        "http-duplicate-second",
        "renamed-paper.pdf",
        pdf,
    )
    .await;

    let first_status = first.status();
    let second_status = second.status();
    let second_body = to_bytes(second.into_body(), usize::MAX)
        .await
        .expect("failed to read duplicate response");
    let second_json: serde_json::Value =
        serde_json::from_slice(&second_body).expect("response should be JSON");
    let paper_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers WHERE uploaded_by = $1")
        .bind(user_id)
        .fetch_one(&db)
        .await
        .expect("failed to count papers");
    let puts = store.put_count();
    let deletes = store.delete_count();
    let grobid_calls = *request_count.lock().expect("request count lock poisoned");

    cleanup_user(&db, user_id).await;
    server.abort();

    assert_eq!(first_status, StatusCode::CREATED);
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(second_json["status"], "already_imported");
    assert_eq!(paper_count.0, 1);
    assert_eq!(puts, 1);
    assert_eq!(deletes, 0);
    assert_eq!(grobid_calls, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn grobid_http_failures_return_502_and_rollback() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, session_token) = create_test_user_and_session(&db).await;
    let (grobid_url, request_count, server) = start_mock_grobid(vec![
        (StatusCode::NO_CONTENT, String::new()),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "GROBID unavailable".to_string(),
        ),
        (StatusCode::OK, "not BibTeX".to_string()),
    ])
    .await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));

    let cases = [
        (
            "grobid-no-content",
            "no-content.pdf",
            b"%PDF-1.7\nno content response\n".as_slice(),
        ),
        (
            "grobid-server-error",
            "server-error.pdf",
            b"%PDF-1.7\nserver error response\n".as_slice(),
        ),
        (
            "grobid-invalid-bibtex",
            "invalid-bibtex.pdf",
            b"%PDF-1.7\ninvalid BibTeX response\n".as_slice(),
        ),
    ];

    let mut statuses = Vec::new();
    let mut error_codes = Vec::new();
    for (boundary, filename, pdf) in cases {
        let response = send_pdf_request(app.clone(), &session_token, boundary, filename, pdf).await;
        statuses.push(response.status());
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("failed to read error response");
        let json: serde_json::Value =
            serde_json::from_slice(&body).expect("error response should be JSON");
        error_codes.push(json["error"].as_str().map(ToString::to_string));
    }

    let paper_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers WHERE uploaded_by = $1")
        .bind(user_id)
        .fetch_one(&db)
        .await
        .expect("failed to count papers");
    let puts = store.put_count();
    let deletes = store.delete_count();
    let grobid_calls = *request_count.lock().expect("request count lock poisoned");

    cleanup_user(&db, user_id).await;
    server.abort();

    assert_eq!(statuses, vec![StatusCode::BAD_GATEWAY; 3]);
    assert_eq!(error_codes, vec![Some("grobid_error".to_string()); 3]);
    assert_eq!(paper_count.0, 0);
    assert_eq!(puts, 3);
    assert_eq!(deletes, 3);
    assert_eq!(grobid_calls, 3);
}

#[tokio::test(flavor = "current_thread")]
async fn paper_import_without_minimum_metadata_returns_metadata_required() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, session_token) = create_test_user_and_session(&db).await;
    let empty_bibtex = "@misc{metadata_missing,\n}";
    let (grobid_url, request_count, server) =
        start_mock_grobid(vec![(StatusCode::OK, empty_bibtex.to_string())]).await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));

    let response = send_pdf_request(
        app,
        &session_token,
        "metadata-required-boundary",
        "metadata-missing.pdf",
        b"%PDF-1.7\nmetadata missing paper\n",
    )
    .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response should be JSON");
    assert_eq!(json["status"], "metadata_required");
    assert_eq!(json["requires_bibliographic_input"], true);
    assert!(json["doi"].is_null());
    assert!(json["title"].is_null());

    let paper_id = Uuid::parse_str(json["paper_id"].as_str().expect("paper_id string"))
        .expect("paper_id UUID");
    let saved: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT doi, title FROM papers WHERE id = $1")
            .bind(paper_id)
            .fetch_one(&db)
            .await
            .expect("metadata-required paper must be saved");
    assert_eq!(saved, (None, None));
    assert_eq!(store.put_count(), 1);
    assert_eq!(
        *request_count.lock().expect("request count lock poisoned"),
        1
    );

    cleanup_user(&db, user_id).await;
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_user_can_complete_bibliographic_metadata_through_app() {
    let db = test_db_pool().await;
    let (owner, token) = create_test_user_and_session(&db).await;
    let paper_id = insert_test_paper(&db, owner, None, None).await;
    let app = paper_import::router(test_state(db.clone(), RecordingObjectStore::default()));

    let response = app
        .oneshot(bibliographic_patch_request(
            &paper_id.to_string(),
            Some(&token),
            serde_json::json!({
                "doi": " https://doi.org/10.7777/http.example ",
                "title": null
            }),
        ))
        .await
        .expect("bibliographic PATCH response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response should be JSON");
    assert_eq!(json["paper_id"], paper_id.to_string());
    assert_eq!(json["doi"], "10.7777/http.example");
    assert!(json["title"].is_null());
    assert_eq!(json["requires_bibliographic_input"], false);

    let saved: (Option<String>,) = sqlx::query_as("SELECT doi FROM papers WHERE id = $1")
        .bind(paper_id)
        .fetch_one(&db)
        .await
        .expect("completed DOI should be saved");
    assert_eq!(saved.0.as_deref(), Some("10.7777/http.example"));
    cleanup_user(&db, owner).await;
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_non_uploader_can_complete_bibliographic_metadata_through_app() {
    let db = test_db_pool().await;
    let (uploader, _uploader_token) = create_test_user_and_session(&db).await;
    let (other_user, other_token) = create_test_user_and_session(&db).await;
    let paper_id = insert_test_paper(&db, uploader, None, None).await;
    let app = paper_import::router(test_state(db.clone(), RecordingObjectStore::default()));

    let response = app
        .oneshot(bibliographic_patch_request(
            &paper_id.to_string(),
            Some(&other_token),
            serde_json::json!({"title": "Shared paper title"}),
        ))
        .await
        .expect("bibliographic PATCH response");

    assert_eq!(response.status(), StatusCode::OK);
    let saved: (Option<String>,) = sqlx::query_as("SELECT title FROM papers WHERE id = $1")
        .bind(paper_id)
        .fetch_one(&db)
        .await
        .expect("completed title should be saved");
    assert_eq!(saved.0.as_deref(), Some("Shared paper title"));

    cleanup_user(&db, uploader).await;
    cleanup_user(&db, other_user).await;
}

#[tokio::test(flavor = "current_thread")]
async fn bibliographic_metadata_route_rejects_invalid_requests() {
    let db = test_db_pool().await;
    let (owner, owner_token) = create_test_user_and_session(&db).await;
    let paper_id = insert_test_paper(&db, owner, None, None).await;
    let app = paper_import::router(test_state(db.clone(), RecordingObjectStore::default()));

    let cases = [
        (
            paper_id.to_string(),
            None,
            serde_json::json!({"title": "No session"}),
            StatusCode::UNAUTHORIZED,
        ),
        (
            "not-a-uuid".to_string(),
            Some(owner_token.as_str()),
            serde_json::json!({"title": "Invalid UUID"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            paper_id.to_string(),
            Some(owner_token.as_str()),
            serde_json::json!({}),
            StatusCode::BAD_REQUEST,
        ),
        (
            Uuid::new_v4().to_string(),
            Some(owner_token.as_str()),
            serde_json::json!({"title": "Missing paper"}),
            StatusCode::NOT_FOUND,
        ),
    ];

    for (requested_id, token, body, expected_status) in cases {
        let response = app
            .clone()
            .oneshot(bibliographic_patch_request(&requested_id, token, body))
            .await
            .expect("bibliographic error response");
        assert_eq!(response.status(), expected_status);
    }

    let saved: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT doi, title FROM papers WHERE id = $1")
            .bind(paper_id)
            .fetch_one(&db)
            .await
            .expect("paper should remain unchanged");
    assert_eq!(saved, (None, None));
    cleanup_user(&db, owner).await;
}

#[tokio::test(flavor = "current_thread")]
async fn bibliographic_metadata_route_preserves_existing_values() {
    let db = test_db_pool().await;
    let (owner, token) = create_test_user_and_session(&db).await;
    let paper_id =
        insert_test_paper(&db, owner, Some("10.1000/grobid"), Some("GROBID title")).await;
    let app = paper_import::router(test_state(db.clone(), RecordingObjectStore::default()));

    let response = app
        .oneshot(bibliographic_patch_request(
            &paper_id.to_string(),
            Some(&token),
            serde_json::json!({
                "doi": "10.1000/replacement",
                "title": "Replacement title"
            }),
        ))
        .await
        .expect("bibliographic PATCH response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response should be JSON");
    assert_eq!(json["doi"], "10.1000/grobid");
    assert_eq!(json["title"], "GROBID title");
    let saved: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT doi, title FROM papers WHERE id = $1")
            .bind(paper_id)
            .fetch_one(&db)
            .await
            .expect("saved metadata should be queryable");
    assert_eq!(saved.0.as_deref(), Some("10.1000/grobid"));
    assert_eq!(saved.1.as_deref(), Some("GROBID title"));

    cleanup_user(&db, owner).await;
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_pdf_without_metadata_returns_metadata_required_ok() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let (user_id, token) = create_test_user_and_session(&db).await;
    let (grobid_url, request_count, server) = start_mock_grobid(vec![(
        StatusCode::OK,
        "@misc{metadata_missing,\n}".to_string(),
    )])
    .await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let store = RecordingObjectStore::default();
    let app = paper_import::router(test_state(db.clone(), store.clone()));
    let pdf = format!("%PDF-1.7\nmetadata-required duplicate {user_id}\n").into_bytes();

    let first = send_pdf_request(
        app.clone(),
        &token,
        "metadata-required-first",
        "paper.pdf",
        &pdf,
    )
    .await;
    let second =
        send_pdf_request(app, &token, "metadata-required-second", "renamed.pdf", &pdf).await;

    assert_eq!(first.status(), StatusCode::CREATED);
    assert_eq!(second.status(), StatusCode::OK);
    let body = to_bytes(second.into_body(), usize::MAX)
        .await
        .expect("failed to read duplicate response");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response should be JSON");
    assert_eq!(json["status"], "metadata_required");
    assert_eq!(json["requires_bibliographic_input"], true);
    assert_eq!(store.put_count(), 1);
    assert_eq!(
        *request_count.lock().expect("request count lock poisoned"),
        1
    );

    cleanup_user(&db, user_id).await;
    server.abort();
}
