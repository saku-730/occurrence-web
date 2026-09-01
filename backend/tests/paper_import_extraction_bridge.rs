use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::body::Bytes;
use backend::features::{
    media::service::{
        DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectByteStream, MediaObjectStore,
        MediaServiceError, PutMediaObjectInput,
    },
    paper_import::{
        extraction::{
            PaperOccurrenceExtractionError, PaperOccurrenceExtractionService,
            PaperOccurrenceExtractor,
        },
        llama::{
            LlamaError, OccurrenceCandidate, OccurrenceExtractionResult, PaperLlmExtractionError,
        },
    },
};
use futures_util::stream;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

const PDF_BYTES: &[u8] = b"%PDF-1.7\nbridge-test-pdf\n%%EOF\n";

#[derive(Clone)]
struct FakeObjectStore {
    bytes: Vec<u8>,
    gets: Arc<Mutex<Vec<(String, String)>>>,
    fail_get: bool,
    stream_error_after_first_chunk: bool,
}

impl FakeObjectStore {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            gets: Arc::new(Mutex::new(Vec::new())),
            fail_get: false,
            stream_error_after_first_chunk: false,
        }
    }

    fn failing_get() -> Self {
        Self {
            bytes: Vec::new(),
            gets: Arc::new(Mutex::new(Vec::new())),
            fail_get: true,
            stream_error_after_first_chunk: false,
        }
    }

    fn failing_stream(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            gets: Arc::new(Mutex::new(Vec::new())),
            fail_get: false,
            stream_error_after_first_chunk: true,
        }
    }

    fn get_requests(&self) -> Vec<(String, String)> {
        self.gets.lock().expect("get lock poisoned").clone()
    }
}

#[async_trait::async_trait]
impl MediaObjectStore for FakeObjectStore {
    async fn put_object(&self, _input: PutMediaObjectInput) -> Result<(), MediaServiceError> {
        unreachable!("bridge test must not upload objects")
    }

    async fn get_object(
        &self,
        input: GetMediaObjectInput,
    ) -> Result<MediaObjectByteStream, MediaServiceError> {
        self.gets
            .lock()
            .expect("get lock poisoned")
            .push((input.bucket, input.object_key));

        if self.fail_get {
            return Err(MediaServiceError::ObjectStoreFailed);
        }

        let midpoint = self.bytes.len() / 2;
        let mut chunks = vec![
            Ok(Bytes::copy_from_slice(&self.bytes[..midpoint])),
            Ok(Bytes::copy_from_slice(&self.bytes[midpoint..])),
        ];
        if self.stream_error_after_first_chunk {
            chunks[1] = Err(MediaServiceError::ObjectStoreFailed);
        }
        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn delete_object(&self, _input: DeleteMediaObjectInput) -> Result<(), MediaServiceError> {
        unreachable!("bridge test must not delete objects")
    }
}

#[derive(Clone)]
struct RecordingExtractor {
    calls: Arc<Mutex<Vec<Vec<u8>>>>,
    paths: Arc<Mutex<Vec<PathBuf>>>,
    fail: bool,
}

impl RecordingExtractor {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            paths: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            paths: Arc::new(Mutex::new(Vec::new())),
            fail: true,
        }
    }

    fn calls(&self) -> Vec<Vec<u8>> {
        self.calls.lock().expect("extractor lock poisoned").clone()
    }

    fn paths(&self) -> Vec<PathBuf> {
        self.paths.lock().expect("extractor path lock poisoned").clone()
    }
}

#[async_trait::async_trait]
impl PaperOccurrenceExtractor for RecordingExtractor {
    async fn extract(
        &self,
        pdf_path: &Path,
    ) -> Result<OccurrenceExtractionResult, PaperLlmExtractionError> {
        let bytes = tokio::fs::read(pdf_path)
            .await
            .expect("temporary staged PDF should be readable");
        self.calls
            .lock()
            .expect("extractor lock poisoned")
            .push(bytes);

        self.paths
            .lock()
            .expect("extractor path lock poisoned")
            .push(pdf_path.to_path_buf());

        if self.fail {
            return Err(PaperLlmExtractionError::Llama(LlamaError::RequestFailed));
        }

        Ok(OccurrenceExtractionResult {
            occurrences: vec![OccurrenceCandidate {
                scientific_name: "Metaphire hilgendorfi".to_string(),
                locality: Some("Tokyo".to_string()),
                decimal_latitude: Some(35.0),
                decimal_longitude: Some(139.0),
            }],
        })
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

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

async fn create_staged_import(db: &PgPool, expected_bytes: &[u8]) -> (Uuid, Uuid, String, String) {
    let user_id = Uuid::new_v4();
    let import_id = Uuid::new_v4();
    let reserved_paper_id = Uuid::new_v4();
    let bucket = format!("paper-bridge-test-{import_id}");
    let object_key = format!("papers/{reserved_paper_id}/original.pdf");

    sqlx::query(
        r#"
        INSERT INTO users (id, email, user_name, password_hash)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(user_id)
    .bind(format!("paper-bridge-{user_id}@example.invalid"))
    .bind(format!("paper-bridge-{user_id}"))
    .bind("test-password-hash")
    .execute(db)
    .await
    .expect("test user should be inserted");

    sqlx::query(
        r#"
        INSERT INTO paper_imports (
            id, reserved_paper_id, bucket, object_key, content_type,
            size_bytes, original_filename, sha256, title, uploaded_by, status
        )
        VALUES ($1, $2, $3, $4, 'application/pdf', $5, 'bridge.pdf', $6, $7, $8, 'staged')
        "#,
    )
    .bind(import_id)
    .bind(reserved_paper_id)
    .bind(&bucket)
    .bind(&object_key)
    .bind(expected_bytes.len() as i64)
    .bind(sha256_hex(expected_bytes))
    .bind("Bridge test paper")
    .bind(user_id)
    .execute(db)
    .await
    .expect("staged paper import should be inserted");

    (import_id, user_id, bucket, object_key)
}

async fn cleanup(db: &PgPool, import_id: Uuid, user_id: Uuid) {
    sqlx::query("DELETE FROM paper_imports WHERE id = $1")
        .bind(import_id)
        .execute(db)
        .await
        .expect("paper import cleanup should succeed");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(db)
        .await
        .expect("user cleanup should succeed");
}

#[tokio::test]
async fn service_downloads_staged_pdf_and_passes_it_to_extractor() {
    let db = test_db_pool().await;
    let (import_id, user_id, bucket, object_key) = create_staged_import(&db, PDF_BYTES).await;
    let store = FakeObjectStore::new(PDF_BYTES.to_vec());
    let extractor = RecordingExtractor::new();

    let output = PaperOccurrenceExtractionService::extract(
        import_id,
        user_id,
        &store,
        &extractor,
        &db,
    )
    .await
    .expect("staged PDF should reach occurrence extractor");

    assert_eq!(output.import_id, import_id);
    assert_eq!(output.result.occurrences.len(), 1);
    assert_eq!(output.result.occurrences[0].scientific_name, "Metaphire hilgendorfi");
    assert_eq!(store.get_requests(), vec![(bucket, object_key)]);
    assert_eq!(extractor.calls(), vec![PDF_BYTES.to_vec()]);

    let status: String = sqlx::query_scalar("SELECT status FROM paper_imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&db)
        .await
        .expect("paper import status should be readable");
    assert_eq!(status, "reviewing");

    cleanup(&db, import_id, user_id).await;
}

#[tokio::test]
async fn service_rejects_corrupted_garage_pdf_and_returns_import_to_staged() {
    let db = test_db_pool().await;
    let (import_id, user_id, _, _) = create_staged_import(&db, PDF_BYTES).await;
    let store = FakeObjectStore::new(b"%PDF-1.7\ncorrupted-object\n%%EOF\n".to_vec());
    let extractor = RecordingExtractor::new();

    let error = PaperOccurrenceExtractionService::extract(
        import_id,
        user_id,
        &store,
        &extractor,
        &db,
    )
    .await
    .expect_err("Garage object that differs from staged metadata must be rejected");

    assert!(matches!(
        error,
        PaperOccurrenceExtractionError::InvalidStoredPdf
    ));
    assert!(extractor.calls().is_empty());

    let status: String = sqlx::query_scalar("SELECT status FROM paper_imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(&db)
        .await
        .expect("paper import status should be readable");
    assert_eq!(status, "staged");

    cleanup(&db, import_id, user_id).await;
}

async fn import_status(db: &PgPool, import_id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM paper_imports WHERE id = $1")
        .bind(import_id)
        .fetch_one(db)
        .await
        .expect("paper import status should be readable")
}

#[tokio::test]
async fn service_restores_staged_after_object_store_or_extractor_failure() {
    let db = test_db_pool().await;

    let (import_id, user_id, _, _) = create_staged_import(&db, PDF_BYTES).await;
    let store = FakeObjectStore::failing_get();
    let extractor = RecordingExtractor::new();
    let error = PaperOccurrenceExtractionService::extract(
        import_id,
        user_id,
        &store,
        &extractor,
        &db,
    )
    .await
    .expect_err("object-store failure should abort extraction");
    assert!(matches!(error, PaperOccurrenceExtractionError::ObjectStoreFailed));
    assert!(extractor.calls().is_empty());
    assert_eq!(import_status(&db, import_id).await, "staged");
    cleanup(&db, import_id, user_id).await;

    let (import_id, user_id, _, _) = create_staged_import(&db, PDF_BYTES).await;
    let store = FakeObjectStore::failing_stream(PDF_BYTES.to_vec());
    let extractor = RecordingExtractor::new();
    let error = PaperOccurrenceExtractionService::extract(
        import_id,
        user_id,
        &store,
        &extractor,
        &db,
    )
    .await
    .expect_err("mid-stream object-store failure should abort extraction");
    assert!(matches!(error, PaperOccurrenceExtractionError::ObjectStoreFailed));
    assert!(extractor.calls().is_empty());
    assert_eq!(import_status(&db, import_id).await, "staged");
    cleanup(&db, import_id, user_id).await;

    let (import_id, user_id, _, _) = create_staged_import(&db, PDF_BYTES).await;
    let store = FakeObjectStore::new(PDF_BYTES.to_vec());
    let extractor = RecordingExtractor::failing();
    let error = PaperOccurrenceExtractionService::extract(
        import_id,
        user_id,
        &store,
        &extractor,
        &db,
    )
    .await
    .expect_err("extractor failure should abort extraction");
    assert!(matches!(error, PaperOccurrenceExtractionError::Extractor(_)));
    assert_eq!(extractor.calls(), vec![PDF_BYTES.to_vec()]);
    assert_eq!(import_status(&db, import_id).await, "staged");
    cleanup(&db, import_id, user_id).await;
}

#[tokio::test]
async fn service_rejects_other_user_or_non_staged_import_without_reading_pdf() {
    let db = test_db_pool().await;
    let (import_id, user_id, _, _) = create_staged_import(&db, PDF_BYTES).await;
    let store = FakeObjectStore::new(PDF_BYTES.to_vec());
    let extractor = RecordingExtractor::new();

    let error = PaperOccurrenceExtractionService::extract(
        import_id,
        Uuid::new_v4(),
        &store,
        &extractor,
        &db,
    )
    .await
    .expect_err("another user must not start extraction");
    assert!(matches!(error, PaperOccurrenceExtractionError::NotFound));
    assert!(store.get_requests().is_empty());
    assert!(extractor.calls().is_empty());
    assert_eq!(import_status(&db, import_id).await, "staged");

    sqlx::query("UPDATE paper_imports SET status = 'reviewing' WHERE id = $1")
        .bind(import_id)
        .execute(&db)
        .await
        .expect("test import status should be updated");
    let error = PaperOccurrenceExtractionService::extract(
        import_id,
        user_id,
        &store,
        &extractor,
        &db,
    )
    .await
    .expect_err("only staged imports may start extraction");
    assert!(matches!(error, PaperOccurrenceExtractionError::NotFound));
    assert!(store.get_requests().is_empty());
    assert!(extractor.calls().is_empty());

    cleanup(&db, import_id, user_id).await;
}

#[tokio::test]
async fn service_rejects_non_pdf_signature_and_returns_import_to_staged() {
    let db = test_db_pool().await;
    let non_pdf_bytes = b"not a PDF despite matching metadata";
    let (import_id, user_id, _, _) = create_staged_import(&db, non_pdf_bytes).await;
    let store = FakeObjectStore::new(non_pdf_bytes.to_vec());
    let extractor = RecordingExtractor::new();

    let error = PaperOccurrenceExtractionService::extract(
        import_id,
        user_id,
        &store,
        &extractor,
        &db,
    )
    .await
    .expect_err("non-PDF signature must not reach the extractor");

    assert!(matches!(error, PaperOccurrenceExtractionError::InvalidStoredPdf));
    assert!(extractor.calls().is_empty());
    assert_eq!(import_status(&db, import_id).await, "staged");
    cleanup(&db, import_id, user_id).await;
}

#[tokio::test]
async fn service_removes_temporary_pdf_after_extraction() {
    let db = test_db_pool().await;
    let (import_id, user_id, _, _) = create_staged_import(&db, PDF_BYTES).await;
    let store = FakeObjectStore::new(PDF_BYTES.to_vec());
    let extractor = RecordingExtractor::new();

    PaperOccurrenceExtractionService::extract(import_id, user_id, &store, &extractor, &db)
        .await
        .expect("extraction should succeed");

    let paths = extractor.paths();
    assert_eq!(paths.len(), 1);
    assert!(
        !paths[0].exists(),
        "temporary downloaded PDF must be removed after extractor returns"
    );
    cleanup(&db, import_id, user_id).await;
}
