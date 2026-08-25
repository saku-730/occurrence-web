use std::{
    collections::VecDeque,
    ffi::OsString,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::{ACCEPT, CONTENT_TYPE}},
    response::{IntoResponse, Response},
    routing::post,
};
use backend::features::{
    media::service::{
        DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectByteStream, MediaObjectStore,
        MediaServiceError, PutMediaObjectInput,
    },
    paper_import::{
        grobid::GrobidClient,
        repository::PaperRepository,
        service::{
            ImportPaperPdfInput, ImportPaperPdfStatus, PaperImportService, PaperImportServiceError,
        },
    },
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

const BIBTEX: &str = r#"@article{sample,
  author = {Doe, Jane and Smith, John Q.},
  title = {A study of {DNA} in earthworms},
  journal = {Example Journal},
  year = {2025},
  volume = {12},
  number = {3},
  pages = {101--115},
  doi = {https://doi.org/10.1234/example.1},
  eid = {e12345}
}"#;

#[derive(Clone, Debug)]
struct CapturedGrobidRequest {
    accept: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

#[derive(Clone)]
struct MockGrobidState {
    responses: Arc<Mutex<VecDeque<(StatusCode, String)>>>,
    requests: Arc<Mutex<Vec<CapturedGrobidRequest>>>,
}

async fn mock_grobid_handler(
    State(state): State<MockGrobidState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    state
        .requests
        .lock()
        .expect("mock request lock poisoned")
        .push(CapturedGrobidRequest {
            accept: headers
                .get(ACCEPT)
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string),
            content_type: headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string),
            body: body.to_vec(),
        });

    let (status, response_body) = state
        .responses
        .lock()
        .expect("mock response lock poisoned")
        .pop_front()
        .unwrap_or((StatusCode::OK, BIBTEX.to_string()));

    (status, response_body).into_response()
}

async fn start_mock_grobid(
    responses: Vec<(StatusCode, String)>,
) -> (
    String,
    Arc<Mutex<Vec<CapturedGrobidRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    let state = MockGrobidState {
        responses: Arc::new(Mutex::new(responses.into())),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let requests = state.requests.clone();

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

    (format!("http://{address}"), requests, handle)
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

#[derive(Clone, Debug)]
struct RecordedPut {
    bucket: String,
    object_key: String,
    content_type: String,
    payload_sha256: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Default)]
struct RecordingObjectStore {
    puts: Arc<Mutex<Vec<RecordedPut>>>,
    deletes: Arc<Mutex<Vec<(String, String)>>>,
}

impl RecordingObjectStore {
    fn put_count(&self) -> usize {
        self.puts.lock().expect("puts lock poisoned").len()
    }

    fn delete_count(&self) -> usize {
        self.deletes.lock().expect("deletes lock poisoned").len()
    }

    fn first_put(&self) -> RecordedPut {
        self.puts
            .lock()
            .expect("puts lock poisoned")
            .first()
            .expect("expected a Garage PUT")
            .clone()
    }
}

#[async_trait::async_trait]
impl MediaObjectStore for RecordingObjectStore {
    async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
        let bytes = tokio::fs::read(&input.file_path)
            .await
            .map_err(MediaServiceError::FileSystem)?;
        self.puts
            .lock()
            .expect("puts lock poisoned")
            .push(RecordedPut {
                bucket: input.bucket,
                object_key: input.object_key,
                content_type: input.content_type,
                payload_sha256: input.payload_sha256,
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

    async fn delete_object(&self, input: DeleteMediaObjectInput) -> Result<(), MediaServiceError> {
        self.deletes
            .lock()
            .expect("deletes lock poisoned")
            .push((input.bucket, input.object_key));
        Ok(())
    }
}

async fn test_db_pool() -> PgPool {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in the environment or .env for paper import tests");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect test PostgreSQL")
}

async fn create_test_user(db: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO users (id, email, user_name, password_hash)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(format!("paper-import-{id}@example.com"))
    .bind(format!("paper-import-{id}"))
    .bind("test-password-hash")
    .execute(db)
    .await
    .expect("failed to create test user");
    id
}

fn create_pdf(bytes: &[u8]) -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new()
        .prefix("paper-import-test-")
        .suffix(".pdf")
        .tempfile()
        .expect("failed to create test PDF");
    std::fs::write(file.path(), bytes).expect("failed to write test PDF");
    file
}

fn import_input(
    user_id: Uuid,
    file: &tempfile::NamedTempFile,
    sha256: &str,
) -> ImportPaperPdfInput {
    let size_bytes = std::fs::metadata(file.path())
        .expect("test PDF metadata")
        .len();
    ImportPaperPdfInput {
        bucket: "occurrence-media".to_string(),
        uploaded_by: user_id,
        original_filename: Some("paper.pdf".to_string()),
        content_type: "application/pdf".to_string(),
        file_path: file.path().to_path_buf(),
        size_bytes,
        payload_sha256: sha256.to_string(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn grobid_client_sends_expected_request_and_parses_all_metadata() {
    let _env_lock = env_lock();
    let (base_url, requests, server) =
        start_mock_grobid(vec![(StatusCode::OK, BIBTEX.to_string())]).await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &base_url);

    let pdf_bytes = b"%PDF-1.7\nmock scientific paper\n";
    let pdf = create_pdf(pdf_bytes);
    let client = GrobidClient::from_env().expect("GROBID client should initialize");
    let metadata = client
        .extract_header(pdf.path(), pdf_bytes.len() as u64)
        .await
        .expect("GROBID header extraction should succeed");

    assert_eq!(metadata.doi.as_deref(), Some("10.1234/example.1"));
    assert_eq!(metadata.title.as_deref(), Some("A study of DNA in earthworms"));
    assert_eq!(metadata.authors.as_deref(), Some("Doe, Jane; Smith, John Q."));
    assert_eq!(metadata.publication_year, Some(2025));
    assert_eq!(metadata.journal.as_deref(), Some("Example Journal"));
    assert_eq!(metadata.volume.as_deref(), Some("12"));
    assert_eq!(metadata.issue.as_deref(), Some("3"));
    assert_eq!(metadata.pages.as_deref(), Some("101-115"));
    assert_eq!(metadata.article_number.as_deref(), Some("e12345"));

    let captured = requests
        .lock()
        .expect("requests lock poisoned")
        .first()
        .expect("GROBID request should be captured")
        .clone();
    assert_eq!(captured.accept.as_deref(), Some("application/x-bibtex"));
    assert!(
        captured
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
    );
    let request_body = String::from_utf8_lossy(&captured.body);
    assert!(request_body.contains("name=\"input\"; filename=\"paper.pdf\""));
    assert!(request_body.contains("Content-Type: application/pdf"));
    assert!(request_body.contains("%PDF-1.7"));
    assert!(request_body.contains("name=\"consolidateHeader\""));
    assert!(request_body.contains("\r\n\r\n0\r\n"));

    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn new_import_persists_metadata_and_global_duplicate_short_circuits() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let first_user = create_test_user(&db).await;
    let second_user = create_test_user(&db).await;
    let sha256 = "a".repeat(64);
    sqlx::query("DELETE FROM papers WHERE sha256 = $1")
        .bind(&sha256)
        .execute(&db)
        .await
        .expect("failed to clean test paper");

    let (base_url, requests, server) =
        start_mock_grobid(vec![(StatusCode::OK, BIBTEX.to_string())]).await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &base_url);
    let store = RecordingObjectStore::default();
    let pdf_bytes = b"%PDF-1.7\nunique paper for import test\n";
    let pdf = create_pdf(pdf_bytes);

    let first = PaperImportService::import_pdf(
        import_input(first_user, &pdf, &sha256),
        &store,
        &db,
    )
    .await
    .expect("first paper import should succeed");
    assert_eq!(first.status, ImportPaperPdfStatus::Imported);
    assert_eq!(first.doi.as_deref(), Some("10.1234/example.1"));

    let row = PaperRepository::find_by_sha256(&db, &sha256)
        .await
        .expect("paper lookup should succeed")
        .expect("paper row should exist");
    assert_eq!(row.id, first.paper_id);
    assert_eq!(row.uploaded_by, first_user);
    assert_eq!(row.doi.as_deref(), Some("10.1234/example.1"));
    assert_eq!(row.title.as_deref(), Some("A study of DNA in earthworms"));
    assert_eq!(row.authors.as_deref(), Some("Doe, Jane; Smith, John Q."));
    assert_eq!(row.publication_year, Some(2025));
    assert_eq!(row.journal.as_deref(), Some("Example Journal"));
    assert_eq!(row.volume.as_deref(), Some("12"));
    assert_eq!(row.issue.as_deref(), Some("3"));
    assert_eq!(row.pages.as_deref(), Some("101-115"));
    assert_eq!(row.article_number.as_deref(), Some("e12345"));

    let put = store.first_put();
    assert_eq!(put.bucket, "occurrence-media");
    assert_eq!(put.object_key, format!("papers/{}/original.pdf", first.paper_id));
    assert_eq!(put.content_type, "application/pdf");
    assert_eq!(put.payload_sha256, sha256);
    assert_eq!(put.bytes, pdf_bytes);

    let second = PaperImportService::import_pdf(
        import_input(second_user, &pdf, &"a".repeat(64)),
        &store,
        &db,
    )
    .await
    .expect("duplicate import should return existing paper");
    assert_eq!(second.status, ImportPaperPdfStatus::AlreadyImported);
    assert_eq!(second.paper_id, first.paper_id);
    assert_eq!(store.put_count(), 1, "duplicate must not create another Garage object");
    assert_eq!(store.delete_count(), 0);
    assert_eq!(
        requests.lock().expect("requests lock poisoned").len(),
        1,
        "duplicate must not call GROBID again"
    );

    sqlx::query("DELETE FROM papers WHERE sha256 = $1")
        .bind(&sha256)
        .execute(&db)
        .await
        .expect("failed to delete test paper");
    sqlx::query("DELETE FROM users WHERE id = $1 OR id = $2")
        .bind(first_user)
        .bind(second_user)
        .execute(&db)
        .await
        .expect("failed to delete test users");
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn grobid_failure_rolls_back_garage_and_leaves_no_paper_row() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let user_id = create_test_user(&db).await;
    let sha256 = "b".repeat(64);
    sqlx::query("DELETE FROM papers WHERE sha256 = $1")
        .bind(&sha256)
        .execute(&db)
        .await
        .expect("failed to clean test paper");

    let (base_url, requests, server) = start_mock_grobid(vec![(
        StatusCode::INTERNAL_SERVER_ERROR,
        "GROBID failed".to_string(),
    )])
    .await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &base_url);
    let store = RecordingObjectStore::default();
    let pdf = create_pdf(b"%PDF-1.7\nrollback test\n");

    let result = PaperImportService::import_pdf(
        import_input(user_id, &pdf, &sha256),
        &store,
        &db,
    )
    .await;

    assert!(matches!(result, Err(PaperImportServiceError::Grobid(_))));
    assert_eq!(store.put_count(), 1);
    assert_eq!(store.delete_count(), 1, "Garage object must be rolled back");
    assert_eq!(requests.lock().expect("requests lock poisoned").len(), 1);
    assert!(
        PaperRepository::find_by_sha256(&db, &sha256)
            .await
            .expect("paper lookup should succeed")
            .is_none(),
        "failed GROBID import must not be marked imported"
    );

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&db)
        .await
        .expect("failed to delete test user");
    server.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn database_failure_after_grobid_rolls_back_garage() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let missing_user_id = Uuid::new_v4();
    let sha256 = "c".repeat(64);
    sqlx::query("DELETE FROM papers WHERE sha256 = $1")
        .bind(&sha256)
        .execute(&db)
        .await
        .expect("failed to clean test paper");

    let (base_url, requests, server) =
        start_mock_grobid(vec![(StatusCode::OK, BIBTEX.to_string())]).await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &base_url);
    let store = RecordingObjectStore::default();
    let pdf = create_pdf(b"%PDF-1.7\nforeign key rollback test\n");

    let result = PaperImportService::import_pdf(
        import_input(missing_user_id, &pdf, &sha256),
        &store,
        &db,
    )
    .await;

    assert!(matches!(result, Err(PaperImportServiceError::Database(_))));
    assert_eq!(store.put_count(), 1);
    assert_eq!(store.delete_count(), 1, "DB failure must roll Garage back");
    assert_eq!(requests.lock().expect("requests lock poisoned").len(), 1);
    assert!(
        PaperRepository::find_by_sha256(&db, &sha256)
            .await
            .expect("paper lookup should succeed")
            .is_none()
    );

    server.abort();
}
