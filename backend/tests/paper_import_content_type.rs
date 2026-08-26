use std::{
    ffi::OsString,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use axum::{Router, http::StatusCode, response::IntoResponse, routing::post};
use backend::features::{
    media::service::{
        DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectByteStream, MediaObjectStore,
        MediaServiceError, PutMediaObjectInput,
    },
    paper_import::service::{ImportPaperPdfInput, ImportPaperPdfStatus, PaperImportService},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

const BIBTEX: &str = r#"@article{sample,
  title = {MIME normalization test},
  year = {2026}
}"#;

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

async fn start_mock_grobid() -> (String, tokio::task::JoinHandle<()>) {
    let app = Router::new().route(
        "/api/processHeaderDocument",
        post(|| async { (StatusCode::OK, BIBTEX).into_response() }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock GROBID");
    let address = listener.local_addr().expect("mock GROBID address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock GROBID failed");
    });
    (format!("http://{address}"), handle)
}

#[derive(Clone, Default)]
struct RecordingObjectStore {
    content_types: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl MediaObjectStore for RecordingObjectStore {
    async fn put_object(&self, input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
        self.content_types
            .lock()
            .expect("content type lock poisoned")
            .push(input.content_type);
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

async fn test_db_pool() -> PgPool {
    // Local tests use the same .env loading convention as the application.
    // CI-provided environment variables keep precedence because dotenvy does not overwrite them.
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in the environment or .env for paper import tests");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect test PostgreSQL")
}

#[tokio::test(flavor = "current_thread")]
async fn service_accepts_case_insensitive_pdf_mime_and_stores_canonical_value() {
    let _env_lock = env_lock();
    let db = test_db_pool().await;
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, email, user_name, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(format!("mime-{user_id}@example.com"))
        .bind(format!("mime-{user_id}"))
        .bind("test-password-hash")
        .execute(&db)
        .await
        .expect("failed to create test user");

    let sha256 = "d".repeat(64);
    sqlx::query("DELETE FROM papers WHERE sha256 = $1")
        .bind(&sha256)
        .execute(&db)
        .await
        .expect("failed to clean paper row");

    let (grobid_url, server) = start_mock_grobid().await;
    let _guard = EnvGuard::set("GROBID_BASE_URL", &grobid_url);
    let pdf = tempfile::Builder::new()
        .suffix(".pdf")
        .tempfile()
        .expect("failed to create PDF");
    std::fs::write(pdf.path(), b"%PDF-1.7\ncase insensitive MIME\n").expect("failed to write PDF");
    let size_bytes = std::fs::metadata(pdf.path()).expect("PDF metadata").len();
    let store = RecordingObjectStore::default();

    let output = PaperImportService::import_pdf(
        ImportPaperPdfInput {
            bucket: "occurrence-media".to_string(),
            uploaded_by: user_id,
            original_filename: Some("paper.pdf".to_string()),
            content_type: "Application/PDF".to_string(),
            file_path: pdf.path().to_path_buf(),
            size_bytes,
            payload_sha256: sha256.clone(),
        },
        &store,
        &db,
    )
    .await
    .expect("MIME type matching should be case-insensitive");

    assert_eq!(output.status, ImportPaperPdfStatus::Imported);
    assert_eq!(output.content_type, "application/pdf");
    assert_eq!(
        store
            .content_types
            .lock()
            .expect("content type lock poisoned")
            .as_slice(),
        &["application/pdf".to_string()]
    );

    let stored_content_type: String =
        sqlx::query_scalar("SELECT content_type FROM papers WHERE sha256 = $1")
            .bind(&sha256)
            .fetch_one(&db)
            .await
            .expect("failed to fetch stored content type");
    assert_eq!(stored_content_type, "application/pdf");

    sqlx::query("DELETE FROM papers WHERE sha256 = $1")
        .bind(&sha256)
        .execute(&db)
        .await
        .expect("failed to delete test paper");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&db)
        .await
        .expect("failed to delete test user");
    server.abort();
}
