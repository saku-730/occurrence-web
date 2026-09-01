use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use backend::features::paper_import::{fulltext::GrobidFulltextClient, grobid::GrobidError};

#[derive(Clone)]
struct MockFulltextResponse {
    status: StatusCode,
    body: Arc<str>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug)]
struct CapturedRequest {
    content_type: Option<String>,
    accept: Option<String>,
    body: Vec<u8>,
}

async fn mock_fulltext_handler(
    State(response): State<MockFulltextResponse>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    response
        .requests
        .lock()
        .expect("captured request lock should not be poisoned")
        .push(CapturedRequest {
            content_type: headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned),
            accept: headers
                .get("accept")
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned),
            body: body.to_vec(),
        });

    (response.status, response.body.to_string()).into_response()
}

async fn start_mock_fulltext_grobid(
    status: StatusCode,
    body: &str,
) -> (
    String,
    Arc<Mutex<Vec<CapturedRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock GROBID listener should bind");
    let address = listener
        .local_addr()
        .expect("mock GROBID address should exist");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/api/processFulltextDocument", post(mock_fulltext_handler))
        .with_state(MockFulltextResponse {
            status,
            body: Arc::from(body),
            requests: Arc::clone(&requests),
        });
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock GROBID server should run");
    });

    (format!("http://{address}"), requests, server)
}

fn create_pdf() -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new()
        .suffix(".pdf")
        .tempfile()
        .expect("test PDF should be created");
    std::fs::write(file.path(), b"%PDF-1.7\nfulltext mock paper\n")
        .expect("test PDF should be written");
    file
}

async fn extract(client: &GrobidFulltextClient, path: &Path) -> Result<String, GrobidError> {
    let size = std::fs::metadata(path)
        .expect("test PDF metadata should be readable")
        .len();
    client.extract_tei(path, size).await
}

#[tokio::test]
async fn grobid_fulltext_client_sends_expected_request_and_returns_tei() {
    let tei =
        "<TEI xmlns=\"http://www.tei-c.org/ns/1.0\"><text><body><p>Body</p></body></text></TEI>";
    let (base_url, requests, server) = start_mock_fulltext_grobid(StatusCode::OK, tei).await;
    let client =
        GrobidFulltextClient::from_base_url_with_timeout(&base_url, Duration::from_secs(1))
            .expect("fulltext client should initialize");
    let pdf = create_pdf();

    let result = extract(&client, pdf.path())
        .await
        .expect("TEI response should be returned");

    assert_eq!(result, tei);
    let captured = requests
        .lock()
        .expect("captured request lock should not be poisoned");
    assert_eq!(captured.len(), 1);
    assert!(
        captured[0]
            .content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("multipart/form-data; boundary="))
    );
    assert_eq!(captured[0].accept.as_deref(), Some("application/xml"));
    let body = String::from_utf8_lossy(&captured[0].body);
    assert!(body.contains("name=\"input\"; filename=\"paper.pdf\""));
    assert!(body.contains("Content-Type: application/pdf"));
    assert!(body.contains("name=\"consolidateHeader\"\r\n\r\n0"));
    assert!(body.contains("name=\"consolidateCitations\"\r\n\r\n0"));
    assert!(body.contains("%PDF-1.7"));
    server.abort();
}

#[tokio::test]
async fn grobid_fulltext_client_handles_no_content_and_invalid_tei() {
    let (base_url, _, server) = start_mock_fulltext_grobid(StatusCode::NO_CONTENT, "").await;
    let client =
        GrobidFulltextClient::from_base_url_with_timeout(&base_url, Duration::from_secs(1))
            .expect("fulltext client should initialize");
    let pdf = create_pdf();
    assert!(matches!(
        extract(&client, pdf.path()).await,
        Err(GrobidError::NoContent)
    ));
    server.abort();

    let (base_url, _, server) = start_mock_fulltext_grobid(StatusCode::OK, "not TEI").await;
    let client =
        GrobidFulltextClient::from_base_url_with_timeout(&base_url, Duration::from_secs(1))
            .expect("fulltext client should initialize");
    assert!(matches!(
        extract(&client, pdf.path()).await,
        Err(GrobidError::InvalidResponse)
    ));
    server.abort();
}
