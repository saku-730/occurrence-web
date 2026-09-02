use std::{path::Path, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::preprocess::{
    PaperPdfPreprocessor, PaperPreprocessError, PreprocessedPageImage, PreprocessedPaper,
};

// Temporary hard-coded llama.cpp endpoint. Replace 127.0.0.1 with the actual
// llama.cpp LAN host when deploying this feature.
pub const LLAMA_CHAT_COMPLETIONS_URL: &str =
    "http://127.0.0.1:8080/v1/chat/completions";
pub const LLAMA_MODEL: &str = "local-model";

pub const OCCURRENCE_EXTRACTION_PROMPT: &str = include_str!("prompt.txt");

#[derive(Debug)]
pub enum LlamaError {
    InvalidConfiguration,
    FileSystem(std::io::Error),
    RequestFailed,
    Upstream(StatusCode, String),
    InvalidResponse,
    InvalidOccurrence,
}

impl From<std::io::Error> for LlamaError {
    fn from(error: std::io::Error) -> Self {
        Self::FileSystem(error)
    }
}

#[derive(Debug)]
pub enum PaperLlmExtractionError {
    Preprocess(PaperPreprocessError),
    Llama(LlamaError),
}

impl From<PaperPreprocessError> for PaperLlmExtractionError {
    fn from(error: PaperPreprocessError) -> Self {
        Self::Preprocess(error)
    }
}

impl From<LlamaError> for PaperLlmExtractionError {
    fn from(error: LlamaError) -> Self {
        Self::Llama(error)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OccurrenceCandidate {
    #[serde(rename = "scientificName")]
    pub scientific_name: String,
    pub locality: Option<String>,
    #[serde(rename = "eventDate")]
    pub event_date: Option<String>,
    #[serde(rename = "decimalLatitude")]
    pub decimal_latitude: Option<f64>,
    #[serde(rename = "decimalLongitude")]
    pub decimal_longitude: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OccurrenceExtractionResult {
    pub occurrences: Vec<OccurrenceCandidate>,
}

/// Complete local-PDF to llama.cpp extraction path used by the paper-import
/// pipeline. The preprocessor always renders every PDF page and also keeps the
/// GROBID text when available; both are then sent in one multimodal request.
pub async fn extract_occurrences_from_pdf(
    pdf_path: &Path,
) -> Result<OccurrenceExtractionResult, PaperLlmExtractionError> {
    let paper = PaperPdfPreprocessor::preprocess(pdf_path).await?;
    let llama = LlamaClient::hardcoded()?;
    Ok(llama.extract_occurrences(&paper).await?)
}

pub struct LlamaClient {
    http: Client,
    endpoint: String,
    model: String,
}

impl LlamaClient {
    pub fn hardcoded() -> Result<Self, LlamaError> {
        Self::new(
            LLAMA_CHAT_COMPLETIONS_URL,
            LLAMA_MODEL,
            Duration::from_secs(1800),
        )
    }

    pub fn new(endpoint: &str, model: &str, timeout: Duration) -> Result<Self, LlamaError> {
        let endpoint = endpoint.trim().to_string();
        let parsed = reqwest::Url::parse(&endpoint).map_err(|_| LlamaError::InvalidConfiguration)?;
        let model = model.trim().to_string();
        if endpoint.is_empty()
            || model.is_empty()
            || timeout.is_zero()
            || !matches!(parsed.scheme(), "http" | "https")
        {
            return Err(LlamaError::InvalidConfiguration);
        }

        let http = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| LlamaError::InvalidConfiguration)?;

        Ok(Self {
            http,
            endpoint,
            model,
        })
    }

    /// Send the GROBID-derived text and every rendered PDF page image in one
    /// multimodal llama.cpp chat-completions request.
    pub async fn extract_occurrences(
        &self,
        paper: &PreprocessedPaper,
    ) -> Result<OccurrenceExtractionResult, LlamaError> {
        self.extract_occurrences_from_parts(&paper.text, &paper.page_images)
            .await
    }

    /// Submit already preprocessed text and page images. Keeping this boundary
    /// separate lets the import pipeline own PDF conversion while callers can
    /// reuse the same multimodal request for a staged paper.
    pub async fn extract_occurrences_from_parts(
        &self,
        text: &str,
        page_images: &[PreprocessedPageImage],
    ) -> Result<OccurrenceExtractionResult, LlamaError> {
        let request = build_request_parts(&self.model, text, page_images).await?;
        let response = self
            .http
            .post(&self.endpoint)
            .json(&request)
            .send()
            .await
            .map_err(|_| LlamaError::RequestFailed)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = body.trim().chars().take(2000).collect::<String>();
            return Err(LlamaError::Upstream(status, message));
        }

        let response: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|_| LlamaError::InvalidResponse)?;
        let content = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .filter(|content| !content.trim().is_empty())
            .ok_or(LlamaError::InvalidResponse)?;

        let mut result: OccurrenceExtractionResult =
            serde_json::from_str(&content).map_err(|_| LlamaError::InvalidResponse)?;
        sanitize_event_dates(&mut result);
        validate_occurrences(&result)?;
        Ok(result)
    }
}

async fn build_request_parts(
    model: &str,
    text: &str,
    page_images: &[PreprocessedPageImage],
) -> Result<Value, LlamaError> {
    let mut content = Vec::with_capacity(2 + page_images.len() * 2);
    content.push(json!({
        "type": "text",
        "text": OCCURRENCE_EXTRACTION_PROMPT,
    }));

    let extracted_text = if text.trim().is_empty() {
        "## 論文PDFから抽出したテキスト\n\nテキストは抽出できませんでした。ページ画像を主に参照してください。".to_string()
    } else {
        format!("## 論文PDFから抽出したテキスト\n\n{text}")
    };
    content.push(json!({
        "type": "text",
        "text": extracted_text,
    }));

    for page in page_images {
        let bytes = tokio::fs::read(&page.path).await?;
        if bytes.is_empty() {
            return Err(LlamaError::InvalidResponse);
        }
        let encoded = BASE64_STANDARD.encode(bytes);
        content.push(json!({
            "type": "text",
            "text": format!("## PDF page {}", page.page_number),
        }));
        content.push(json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{};base64,{}", page.media_type, encoded)
            }
        }));
    }

    Ok(json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": content
            }
        ],
        "temperature": 0.7,
        "top_p": 0.8,
        "top_k": 20,
        "min_p": 0.0,
        "presence_penalty": 2.0,
        "repeat_penalty": 1.0,
        "max_tokens": 32768,
        "stream": false,
        "chat_template_kwargs": {
            "enable_thinking": false
        },
        "response_format": occurrence_response_format(),
    }))
}

fn occurrence_response_format() -> Value {
    json!({
        "type": "json_schema",
        "schema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["occurrences"],
            "properties": {
                "occurrences": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["scientificName", "locality", "eventDate"],
                        "properties": {
                            "scientificName": { "type": "string", "minLength": 1 },
                            "locality": { "type": ["string", "null"] },
                            "eventDate": { "type": ["string", "null"] }
                        }
                    }
                }
            }
        }
    })
}

fn sanitize_event_dates(result: &mut OccurrenceExtractionResult) {
    for occurrence in &mut result.occurrences {
        occurrence.event_date = occurrence
            .event_date
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| valid_event_date(value));
    }
}

fn validate_occurrences(result: &OccurrenceExtractionResult) -> Result<(), LlamaError> {
    for occurrence in &result.occurrences {
        if occurrence.scientific_name.trim().is_empty() {
            return Err(LlamaError::InvalidOccurrence);
        }
        if occurrence
            .decimal_latitude
            .is_some_and(|value| !(-90.0..=90.0).contains(&value))
            || occurrence
                .decimal_longitude
                .is_some_and(|value| !(-180.0..=180.0).contains(&value))
        {
            return Err(LlamaError::InvalidOccurrence);
        }
    }
    Ok(())
}

fn valid_event_date(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }

    let mut parts = value.split('/');
    let Some(first) = parts.next() else {
        return false;
    };
    let second = parts.next();
    if parts.next().is_some() {
        return false;
    }

    valid_event_date_part(first) && second.is_none_or(valid_event_date_part)
}

fn valid_event_date_part(value: &str) -> bool {
    let bytes = value.as_bytes();
    match bytes.len() {
        4 => bytes.iter().all(u8::is_ascii_digit),
        7 => {
            bytes[0..4].iter().all(u8::is_ascii_digit)
                && bytes[4] == b'-'
                && bytes[5..7].iter().all(u8::is_ascii_digit)
                && (1..=12).contains(&value[5..7].parse::<u8>().unwrap_or(0))
        }
        10 => {
            bytes[0..4].iter().all(u8::is_ascii_digit)
                && bytes[4] == b'-'
                && bytes[5..7].iter().all(u8::is_ascii_digit)
                && bytes[7] == b'-'
                && bytes[8..10].iter().all(u8::is_ascii_digit)
                && (1..=12).contains(&value[5..7].parse::<u8>().unwrap_or(0))
                && (1..=31).contains(&value[8..10].parse::<u8>().unwrap_or(0))
        }
        _ => false,
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        extract::State,
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::post,
    };

    use super::*;
    use crate::features::paper_import::preprocess::PAPER_PAGE_IMAGE_MEDIA_TYPE;

    #[derive(Clone)]
    struct MockLlamaResponse {
        status: StatusCode,
        body: Value,
        requests: Arc<Mutex<Vec<Value>>>,
    }

    async fn mock_llama_handler(
        State(response): State<MockLlamaResponse>,
        Json(request): Json<Value>,
    ) -> Response {
        response
            .requests
            .lock()
            .expect("mock llama request lock should not be poisoned")
            .push(request);
        (response.status, Json(response.body)).into_response()
    }

    async fn start_mock_llama(
        status: StatusCode,
        body: Value,
    ) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock llama listener should bind");
        let address = listener
            .local_addr()
            .expect("mock llama address should exist");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/chat/completions", post(mock_llama_handler))
            .with_state(MockLlamaResponse {
                status,
                body,
                requests: Arc::clone(&requests),
            });
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock llama server should run");
        });
        tokio::task::yield_now().await;

        (format!("http://{address}/v1/chat/completions"), requests, server)
    }

    async fn test_page_images() -> (tempfile::TempDir, Vec<PreprocessedPageImage>) {
        let directory = tempfile::tempdir().expect("temporary image directory should exist");
        let first = directory.path().join("page-1.jpg");
        let second = directory.path().join("page-2.jpg");
        tokio::fs::write(&first, b"first-image")
            .await
            .expect("first image should be written");
        tokio::fs::write(&second, b"second-image")
            .await
            .expect("second image should be written");
        (
            directory,
            vec![
                PreprocessedPageImage {
                    page_number: 1,
                    path: first,
                    media_type: PAPER_PAGE_IMAGE_MEDIA_TYPE,
                },
                PreprocessedPageImage {
                    page_number: 2,
                    path: second,
                    media_type: PAPER_PAGE_IMAGE_MEDIA_TYPE,
                },
            ],
        )
    }

    fn valid_response() -> Value {
        json!({
            "choices": [{
                "message": {
                    "content": r#"{\"occurrences\":[{\"scientificName\":\"Metaphire hilgendorfi\",\"locality\":\"Tokyo\",\"eventDate\":\"1998-06-04\"}]}"#
                }
            }]
        })
    }

    #[tokio::test]
    async fn multimodal_request_contains_extracted_text_and_page_image() {
        let directory = tempfile::tempdir().expect("temporary image directory should exist");
        let image_path = directory.path().join("page-1.jpg");
        tokio::fs::write(&image_path, b"jpeg-test-bytes")
            .await
            .expect("test image should be written");
        let pages = vec![PreprocessedPageImage {
            page_number: 1,
            path: image_path,
            media_type: PAPER_PAGE_IMAGE_MEDIA_TYPE,
        }];

        let request = build_request_parts(
            "local-model",
            "Metaphire hilgendorfi was collected in Tokyo.",
            &pages,
        )
        .await
        .expect("multimodal request should be constructed");

        let content = request["messages"][0]["content"]
            .as_array()
            .expect("message content should be an array");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], OCCURRENCE_EXTRACTION_PROMPT);
        assert!(OCCURRENCE_EXTRACTION_PROMPT.contains("同じOccurrenceを繰り返し出力しない"));
        assert!(OCCURRENCE_EXTRACTION_PROMPT.contains("直ちに閉じて生成を終了"));
        assert!(OCCURRENCE_EXTRACTION_PROMPT.contains("eventDate"));
        assert_eq!(content[1]["type"], "text");
        assert!(
            content[1]["text"]
                .as_str()
                .expect("extracted text should be a string")
                .contains("Metaphire hilgendorfi")
        );
        assert_eq!(content[2]["text"], "## PDF page 1");
        assert_eq!(content[3]["type"], "image_url");
        assert!(
            content[3]["image_url"]["url"]
                .as_str()
                .expect("image URL should be a string")
                .starts_with("data:image/jpeg;base64,")
        );
        assert_eq!(request["response_format"]["type"], "json_schema");
        let occurrence_schema = &request["response_format"]["schema"]["properties"]
            ["occurrences"]["items"];
        assert_eq!(
            occurrence_schema["required"],
            json!(["scientificName", "locality", "eventDate"])
        );
        assert_eq!(
            occurrence_schema["properties"]["eventDate"]["type"],
            json!(["string", "null"])
        );
        assert!(
            occurrence_schema["properties"]["eventDate"]
                .get("pattern")
                .is_none()
        );
        assert!(
            occurrence_schema["properties"]
                .get("decimalLatitude")
                .is_none()
        );
        assert!(
            occurrence_schema["properties"]
                .get("decimalLongitude")
                .is_none()
        );
    }

    #[tokio::test]
    async fn llama_client_sends_multimodal_request_and_parses_occurrences() {
        let (endpoint, requests, server) = start_mock_llama(StatusCode::OK, valid_response()).await;
        let client = LlamaClient::new(&endpoint, "test-model", Duration::from_secs(1))
            .expect("llama client should initialize");
        let (_directory, pages) = test_page_images().await;

        let result = client
            .extract_occurrences_from_parts("Text extracted by GROBID.", &pages)
            .await
            .expect("valid llama response should be parsed");

        assert_eq!(result.occurrences.len(), 1);
        assert_eq!(result.occurrences[0].scientific_name, "Metaphire hilgendorfi");
        assert_eq!(result.occurrences[0].event_date.as_deref(), Some("1998-06-04"));
        let requests = requests
            .lock()
            .expect("mock llama request lock should not be poisoned");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "test-model");
        assert_eq!(requests[0]["temperature"], 0.7);
        assert_eq!(requests[0]["top_p"], 0.8);
        assert_eq!(requests[0]["top_k"], 20);
        assert_eq!(requests[0]["min_p"], 0.0);
        assert_eq!(requests[0]["presence_penalty"], 2.0);
        assert_eq!(requests[0]["repeat_penalty"], 1.0);
        assert_eq!(requests[0]["max_tokens"], 32768);
        assert_eq!(requests[0]["stream"], false);
        assert_eq!(
            requests[0]["chat_template_kwargs"]["enable_thinking"],
            false
        );
        let content = requests[0]["messages"][0]["content"]
            .as_array()
            .expect("multimodal content should be an array");
        assert_eq!(content.len(), 6);
        assert_eq!(content[0]["text"], OCCURRENCE_EXTRACTION_PROMPT);
        assert!(content[1]["text"]
            .as_str()
            .is_some_and(|text| text.contains("Text extracted by GROBID.")));
        assert_eq!(content[2]["text"], "## PDF page 1");
        assert_eq!(content[3]["image_url"]["url"], "data:image/jpeg;base64,Zmlyc3QtaW1hZ2U=");
        assert_eq!(content[4]["text"], "## PDF page 2");
        assert_eq!(content[5]["image_url"]["url"], "data:image/jpeg;base64,c2Vjb25kLWltYWdl");
        server.abort();
    }

    #[tokio::test]
    async fn multimodal_request_uses_image_fallback_for_empty_text_and_encodes_bytes() {
        let (_directory, pages) = test_page_images().await;

        let request = build_request_parts("test-model", " \n\t", &pages)
            .await
            .expect("request should be built from page images");
        let content = request["messages"][0]["content"]
            .as_array()
            .expect("multimodal content should be an array");

        assert!(content[1]["text"]
            .as_str()
            .is_some_and(|text| text.contains("テキストは抽出できませんでした")));
        assert_eq!(content[3]["image_url"]["url"], "data:image/jpeg;base64,Zmlyc3QtaW1hZU=");
        assert_eq!(content[5]["image_url"]["url"], "data:image/jpeg;base64,c2Vjb25kLWltYWdl");
    }

    #[tokio::test]
    async fn llama_client_rejects_upstream_and_invalid_occurrence_responses() {
        let (_directory, pages) = test_page_images().await;

        let cases = [
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"model overloaded"}),
                "upstream",
            ),
            (StatusCode::OK, json!({"choices":[]}), "invalid_response"),
            (
                StatusCode::OK,
                json!({"choices":[{"message":{"content":"not JSON"}}]}),
                "invalid_response",
            ),
            (
                StatusCode::OK,
                json!({"choices":[{"message":{"content":r#"{\"occurrences\":[{\"scientificName\":\" \",\"locality\":null,\"eventDate\":null,\"decimalLatitude\":null,\"decimalLongitude\":null}]}"#}}]}),
                "invalid_occurrence",
            ),
            (
                StatusCode::OK,
                json!({"choices":[{"message":{"content":r#"{\"occurrences\":[{\"scientificName\":\"A species\",\"locality\":null,\"eventDate\":null,\"decimalLatitude\":91.0,\"decimalLongitude\":0.0}]}"#}}]}),
                "invalid_occurrence",
            ),
        ];

        for (status, body, expected) in cases {
            let (endpoint, _, server) = start_mock_llama(status, body).await;
            let client = LlamaClient::new(&endpoint, "test-model", Duration::from_secs(1))
                .expect("llama client should initialize");
            let error = client
                .extract_occurrences_from_parts("text", &pages)
                .await
                .expect_err("invalid llama response should be rejected");
            match expected {
                "upstream" => assert!(matches!(error, LlamaError::Upstream(StatusCode::INTERNAL_SERVER_ERROR, message) if message.contains("model overloaded"))),
                "invalid_response" => assert!(matches!(error, LlamaError::InvalidResponse)),
                "invalid_occurrence" => assert!(matches!(error, LlamaError::InvalidOccurrence)),
                _ => unreachable!(),
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn llama_client_discards_invalid_event_date_without_losing_occurrence() {
        let (_directory, pages) = test_page_images().await;
        let response = json!({
            "choices": [{
                "message": {
                    "content": r#"{\"occurrences\":[{\"scientificName\":\"Metaphire hilgendorfi\",\"locality\":\"Tokyo\",\"eventDate\":\"1998-13\"}]}"#
                }
            }]
        });
        let (endpoint, _, server) = start_mock_llama(StatusCode::OK, response).await;
        let client = LlamaClient::new(&endpoint, "test-model", Duration::from_secs(1))
            .expect("llama client should initialize");

        let result = client
            .extract_occurrences_from_parts("text", &pages)
            .await
            .expect("invalid event date alone must not fail occurrence extraction");

        assert_eq!(result.occurrences.len(), 1);
        assert_eq!(result.occurrences[0].scientific_name, "Metaphire hilgendorfi");
        assert_eq!(result.occurrences[0].event_date, None);
        server.abort();
    }

    #[tokio::test]
    async fn llama_client_rejects_unknown_occurrence_json_fields() {
        let (_directory, pages) = test_page_images().await;
        let response = json!({
            "choices": [{
                "message": {
                    "content": r#"{\"occurrences\":[{\"scientificName\":\"Metaphire hilgendorfi\",\"locality\":null,\"eventDate\":null,\"decimalLatitude\":null,\"decimalLongitude\":null,\"inventedField\":\"must not be accepted\"}]}"#
                }
            }]
        });
        let (endpoint, _, server) = start_mock_llama(StatusCode::OK, response).await;
        let client = LlamaClient::new(&endpoint, "test-model", Duration::from_secs(1))
            .expect("llama client should initialize");

        let result = client.extract_occurrences_from_parts("text", &pages).await;

        assert!(matches!(result, Err(LlamaError::InvalidResponse)));
        server.abort();
    }

    #[tokio::test]
    async fn multimodal_request_rejects_empty_or_missing_page_image() {
        let directory = tempfile::tempdir().expect("temporary image directory should exist");
        let empty = directory.path().join("empty.jpg");
        tokio::fs::write(&empty, [])
            .await
            .expect("empty image should be written");
        let empty_page = PreprocessedPageImage {
            page_number: 1,
            path: empty,
            media_type: PAPER_PAGE_IMAGE_MEDIA_TYPE,
        };
        assert!(matches!(
            build_request_parts("model", "text", &[empty_page]).await,
            Err(LlamaError::InvalidResponse)
        ));

        let missing_page = PreprocessedPageImage {
            page_number: 2,
            path: directory.path().join("missing.jpg"),
            media_type: PAPER_PAGE_IMAGE_MEDIA_TYPE,
        };
        assert!(matches!(
            build_request_parts("model", "text", &[missing_page]).await,
            Err(LlamaError::FileSystem(_))
        ));
    }

    #[test]
    fn validates_occurrence_coordinate_ranges() {
        let valid = OccurrenceExtractionResult {
            occurrences: vec![OccurrenceCandidate {
                scientific_name: "Metaphire hilgendorfi".to_string(),
                locality: Some("Tokyo".to_string()),
                event_date: Some("1998-06/1998-07".to_string()),
                decimal_latitude: Some(35.0),
                decimal_longitude: Some(139.0),
            }],
        };
        assert!(validate_occurrences(&valid).is_ok());

        let invalid = OccurrenceExtractionResult {
            occurrences: vec![OccurrenceCandidate {
                scientific_name: "Metaphire hilgendorfi".to_string(),
                locality: None,
                event_date: None,
                decimal_latitude: Some(91.0),
                decimal_longitude: None,
            }],
        };
        assert!(matches!(
            validate_occurrences(&invalid),
            Err(LlamaError::InvalidOccurrence)
        ));
    }

    #[test]
    fn sanitizes_event_dates_without_rejecting_occurrence() {
        let mut result = OccurrenceExtractionResult {
            occurrences: vec![
                OccurrenceCandidate {
                    scientific_name: "Metaphire hilgendorfi".to_string(),
                    locality: Some("Tokyo".to_string()),
                    event_date: Some(" 1998-06 ".to_string()),
                    decimal_latitude: None,
                    decimal_longitude: None,
                },
                OccurrenceCandidate {
                    scientific_name: "Amynthas corticis".to_string(),
                    locality: Some("Nara".to_string()),
                    event_date: Some("June 1998".to_string()),
                    decimal_latitude: None,
                    decimal_longitude: None,
                },
            ],
        };

        sanitize_event_dates(&mut result);

        assert_eq!(result.occurrences[0].event_date.as_deref(), Some("1998-06"));
        assert_eq!(result.occurrences[1].event_date, None);
    }

    #[test]
    fn rejects_invalid_client_configuration() {
        assert!(matches!(
            LlamaClient::new("://bad", "model", Duration::from_secs(1)),
            Err(LlamaError::InvalidConfiguration)
        ));
        assert!(matches!(
            LlamaClient::new(
                "http://127.0.0.1:8080/v1/chat/completions",
                "",
                Duration::from_secs(1)
            ),
            Err(LlamaError::InvalidConfiguration)
        ));
    }
}
