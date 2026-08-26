use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::{
        Request, StatusCode,
        header::{CONTENT_TYPE, COOKIE},
    },
};
use backend::{
    app::build_app,
    config::Config,
    features::{
        auth::service::hash_token,
        media::service::{DeleteMediaObjectInput, GetMediaObjectInput, MediaObjectStore},
        occurrences::service::{DarwinCoreTerm, OccurrenceRdfStore, OccurrenceServiceError},
        paper_import,
    },
    infrastructure::garage::GarageMediaObjectStore,
    state::AppState,
};
use futures_util::StreamExt;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

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

async fn create_user_and_session(db: &PgPool) -> (Uuid, String) {
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, email, user_name, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(format!("paper-real-e2e-{user_id}@example.com"))
        .bind(format!("paper-real-e2e-{user_id}"))
        .bind("test-password-hash")
        .execute(db)
        .await
        .expect("failed to create E2E user");

    let session_token = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO sessions (user_id, session_token_hash, expires_at) VALUES ($1, $2, now() + interval '1 day')",
    )
    .bind(user_id)
    .bind(hash_token(&session_token))
    .execute(db)
    .await
    .expect("failed to create E2E session");

    (user_id, session_token)
}

#[tokio::test]
#[ignore = "requires real PostgreSQL, Garage, GROBID, and backend/.env"]
async fn paper_import_route_works_with_real_postgresql_garage_and_grobid() {
    dotenvy::dotenv().ok();
    let config = Config::from_env().expect("integration configuration should be valid");
    let bucket = config.garage.bucket.clone();
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.posgre.url)
        .await
        .expect("failed to connect real PostgreSQL");
    let garage = Arc::new(
        GarageMediaObjectStore::from_env().expect("real Garage configuration should be valid"),
    );
    let object_store: Arc<dyn MediaObjectStore> = garage.clone();
    let state = AppState::new_with_media_object_store(
        config,
        db.clone(),
        Arc::new(NoopOccurrenceRdfStore),
        object_store,
    );
    // Match the production composition in main.rs, where Paper Import is
    // merged separately from the core application router.
    let app = build_app(state.clone()).merge(paper_import::router(state));
    let (user_id, session_token) = create_user_and_session(&db).await;

    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/papers/plos-one-0215794.pdf");
    let mut pdf = std::fs::read(&fixture_path).expect("failed to read research PDF fixture");
    pdf.extend_from_slice(format!("\n% occurrence-web E2E {}\n", Uuid::new_v4()).as_bytes());

    let boundary = "paper-import-real-services-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"research-paper.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(&pdf);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/paper-import")
                .header(
                    CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header(COOKIE, format!("session={session_token}"))
                .body(Body::from(body))
                .expect("failed to build E2E request"),
        )
        .await
        .expect("Paper Import E2E request failed");
    let status = response.status();
    let response_body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read E2E response");
    if status != StatusCode::CREATED {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&db)
            .await
            .expect("failed to clean E2E user after request failure");
        panic!(
            "real Paper Import returned {status}: {}",
            String::from_utf8_lossy(&response_body)
        );
    }

    let json: serde_json::Value =
        serde_json::from_slice(&response_body).expect("E2E response should be JSON");
    let paper_id = Uuid::parse_str(
        json["paper_id"]
            .as_str()
            .expect("paper_id should be returned"),
    )
    .expect("paper_id should be a UUID");
    let object_key = format!("papers/{paper_id}/original.pdf");

    let row: (String, String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT bucket, object_key, title, doi FROM papers WHERE id = $1")
            .bind(paper_id)
            .fetch_one(&db)
            .await
            .expect("real PostgreSQL paper row should exist");

    let mut stream = garage
        .get_object(GetMediaObjectInput {
            bucket: bucket.clone(),
            object_key: object_key.clone(),
        })
        .await
        .expect("real Garage object should be readable");
    let mut stored_pdf = Vec::new();
    while let Some(chunk) = stream.next().await {
        stored_pdf.extend_from_slice(&chunk.expect("Garage stream should remain readable"));
    }

    garage
        .delete_object(DeleteMediaObjectInput {
            bucket: bucket.clone(),
            object_key: object_key.clone(),
        })
        .await
        .expect("real Garage E2E object should be deleted");
    sqlx::query("DELETE FROM papers WHERE id = $1")
        .bind(paper_id)
        .execute(&db)
        .await
        .expect("failed to delete E2E paper row");
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&db)
        .await
        .expect("failed to delete E2E user");

    assert_eq!(row.0, bucket);
    assert_eq!(row.1, object_key);
    assert!(
        row.2
            .as_deref()
            .is_some_and(|title| title.contains("Research applications"))
    );
    assert_eq!(row.3.as_deref(), Some("10.1371/journal.pone.0215794"));
    assert_eq!(stored_pdf, pdf);
}
