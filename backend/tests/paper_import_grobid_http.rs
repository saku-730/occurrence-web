use std::{path::Path, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use backend::features::paper_import::grobid::{GrobidClient, GrobidError};

#[derive(Clone)]
struct MockResponse {
    status: StatusCode,
    body: Arc<str>,
    delay: Duration,
}

async fn mock_grobid_handler(State(response): State<MockResponse>, _body: Bytes) -> Response {
    if !response.delay.is_zero() {
        tokio::time::sleep(response.delay).await;
    }

    (response.status, response.body.to_string()).into_response()
}

async fn start_mock_grobid(
    status: StatusCode,
    body: &str,
    delay: Duration,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock GROBID");
    let address = listener.local_addr().expect("mock GROBID address");
    let app = Router::new()
        .route("/api/processHeaderDocument", post(mock_grobid_handler))
        .with_state(MockResponse {
            status,
            body: Arc::from(body),
            delay,
        });

    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock GROBID server failed");
    });

    (format!("http://{address}"), handle)
}

fn create_pdf() -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new()
        .suffix(".pdf")
        .tempfile()
        .expect("failed to create test PDF");
    std::fs::write(file.path(), b"%PDF-1.7\nmock paper\n").expect("failed to write test PDF");
    file
}

async fn extract(
    client: &GrobidClient,
    path: &Path,
) -> Result<backend::features::paper_import::grobid::GrobidPaperMetadata, GrobidError> {
    let size = std::fs::metadata(path).expect("test PDF metadata").len();
    client.extract_header(path, size).await
}

#[tokio::test]
async fn grobid_client_maps_no_content_response() {
    let (base_url, server) = start_mock_grobid(StatusCode::NO_CONTENT, "", Duration::ZERO).await;
    let client = GrobidClient::from_base_url_with_timeout(&base_url, Duration::from_secs(1))
        .expect("client should initialize");
    let pdf = create_pdf();

    let result = extract(&client, pdf.path()).await;

    assert!(matches!(result, Err(GrobidError::NoContent)));
    server.abort();
}

#[tokio::test]
async fn grobid_client_maps_upstream_error_response() {
    let (base_url, server) =
        start_mock_grobid(StatusCode::INTERNAL_SERVER_ERROR, "failed", Duration::ZERO).await;
    let client = GrobidClient::from_base_url_with_timeout(&base_url, Duration::from_secs(1))
        .expect("client should initialize");
    let pdf = create_pdf();

    let result = extract(&client, pdf.path()).await;

    assert!(matches!(
        result,
        Err(GrobidError::Upstream(StatusCode::INTERNAL_SERVER_ERROR))
    ));
    server.abort();
}

#[tokio::test]
async fn grobid_client_rejects_invalid_bibtex_response() {
    let (base_url, server) = start_mock_grobid(StatusCode::OK, "not BibTeX", Duration::ZERO).await;
    let client = GrobidClient::from_base_url_with_timeout(&base_url, Duration::from_secs(1))
        .expect("client should initialize");
    let pdf = create_pdf();

    let result = extract(&client, pdf.path()).await;

    assert!(matches!(result, Err(GrobidError::InvalidResponse)));
    server.abort();
}

#[tokio::test]
async fn grobid_client_maps_connection_failure() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to reserve unused address");
    let address = listener.local_addr().expect("unused address");
    drop(listener);

    let client = GrobidClient::from_base_url_with_timeout(
        &format!("http://{address}"),
        Duration::from_millis(200),
    )
    .expect("client should initialize");
    let pdf = create_pdf();

    let result = extract(&client, pdf.path()).await;

    assert!(matches!(result, Err(GrobidError::RequestFailed)));
}

#[tokio::test]
async fn grobid_client_times_out_slow_response() {
    let (base_url, server) = start_mock_grobid(
        StatusCode::OK,
        "@article{sample, title={Too late}}",
        Duration::from_millis(250),
    )
    .await;
    let client = GrobidClient::from_base_url_with_timeout(&base_url, Duration::from_millis(25))
        .expect("client should initialize");
    let pdf = create_pdf();

    let result = extract(&client, pdf.path()).await;

    assert!(matches!(result, Err(GrobidError::RequestFailed)));
    server.abort();
}

#[test]
fn grobid_client_rejects_invalid_configuration() {
    assert!(matches!(
        GrobidClient::from_base_url_with_timeout("://invalid", Duration::from_secs(1)),
        Err(GrobidError::InvalidConfiguration)
    ));
    assert!(matches!(
        GrobidClient::from_base_url_with_timeout("ftp://127.0.0.1", Duration::from_secs(1)),
        Err(GrobidError::InvalidConfiguration)
    ));
    assert!(matches!(
        GrobidClient::from_base_url_with_timeout("http://127.0.0.1:8070", Duration::ZERO),
        Err(GrobidError::InvalidConfiguration)
    ));
}
