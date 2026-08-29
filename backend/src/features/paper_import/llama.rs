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

pub const OCCURRENCE_EXTRACTION_PROMPT: &str = r#"入力として、論文PDFから抽出したテキストと、論文のページ画像が与えられます。
これらは同一の論文に由来する情報です。

テキストと画像を別々の情報源として扱うのではなく、両方を相互に参照しながら論文全体を解析してください。

本文、表、図、地図、キャプション、標本リスト、採集地点一覧などから、生物の分類情報と位置情報を抽出してください。

## 抽出ルール

1. 生物の分類情報
- 論文から明示的に読み取れる範囲で、最も下位の特定可能な分類群を使用してください。
- 種まで分かる場合は種を使用してください。
- 種まで分からない場合は、属・科など読み取れる最も具体的な分類群を使用してください。
- 情報を推測して、論文に書かれているより細かい分類群へ補完しないでください。
- 学名について省略名は使用しないでください。

2. 位置情報
- その分類群が実際に記録、採集、観察された地点を抽出してください。
- 地名は locality に記録してください。

3. 分類群と位置情報
- 分類群と、その分類群が記録された位置情報を必ず対応付けてください。
- 本文に分類群があり、対応する地点が表や図にある場合など、テキストと画像を組み合わせて対応関係を判断してください。
- 同じ分類群が複数地点に存在する場合は地点ごとに別レコードにしてください。
- 同じ地点に複数分類群が存在する場合は分類群ごとに別レコードにしてください。
- 対応関係が確認できない場合は推測して組み合わせないでください。

4. 情報源の統合
- テキストと画像に同じ情報が存在する場合、それを重複したOccurrenceとして出力しないでください。
- テキストでは不足している情報を表や図から補える場合は、それらを統合してください。
- 画像では不足している情報を本文やキャプションから補える場合も統合してください。
- 表の行・列、図中のラベル、キャプションの対応関係を崩さないでください。

5. 正確性
- 論文に明示された情報のみを使用してください。
- 推測や創作をしないでください。
- 情報が存在しない項目には null を使用してください。

6. 重複禁止と終了
- scientificName と locality の組み合わせが同一のOccurrenceは、情報源や表現箇所が異なっても1件だけ出力してください。
- 出力前に全レコードを照合し、同一のOccurrenceを統合してください。
- 同じOccurrenceを繰り返し出力しないでください。
- 抽出した重複のないOccurrenceをすべて1回ずつ出力したら、occurrences配列とJSONオブジェクトを直ちに閉じて生成を終了してください。
- occurrences配列を最初から作り直したり、同じJSONを複数回出力したりしないでください。
- JSONの前後に説明、注釈、Markdownのコードブロックを出力しないでください。

## 出力形式

以下のJSONだけを出力してください。

{
  "occurrences": [
    {
      "scientificName": "分類群名",
      "locality": "地点名"
    }
  ]
}

該当する情報が存在しない場合:

{
  "occurrences": []
}"#;

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

        let result: OccurrenceExtractionResult =
            serde_json::from_str(&content).map_err(|_| LlamaError::InvalidResponse)?;
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
        "max_tokens": 8192,
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
                        "required": ["scientificName", "locality"],
                        "properties": {
                            "scientificName": { "type": "string", "minLength": 1 },
                            "locality": { "type": ["string", "null"] }
                        }
                    }
                }
            }
        }
    })
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
                    "content": r#"{"occurrences":[{"scientificName":"Metaphire hilgendorfi","locality":"Tokyo"}]}"#
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
            json!(["scientificName", "locality"])
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
        assert_eq!(requests[0]["max_tokens"], 8192);
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
        assert_eq!(content[3]["image_url"]["url"], "data:image/jpeg;base64,Zmlyc3QtaW1hZ2U=");
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
                json!({"choices":[{"message":{"content":r#"{"occurrences":[{"scientificName":" ","locality":null,"decimalLatitude":null,"decimalLongitude":null}]}"#}}]}),
                "invalid_occurrence",
            ),
            (
                StatusCode::OK,
                json!({"choices":[{"message":{"content":r#"{"occurrences":[{"scientificName":"A species","locality":null,"decimalLatitude":91.0,"decimalLongitude":0.0}]}"#}}]}),
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
    async fn llama_client_rejects_unknown_occurrence_json_fields() {
        let (_directory, pages) = test_page_images().await;
        let response = json!({
            "choices": [{
                "message": {
                    "content": r#"{"occurrences":[{"scientificName":"Metaphire hilgendorfi","locality":null,"decimalLatitude":null,"decimalLongitude":null,"inventedField":"must not be accepted"}]}"#
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
                decimal_latitude: Some(35.0),
                decimal_longitude: Some(139.0),
            }],
        };
        assert!(validate_occurrences(&valid).is_ok());

        let invalid = OccurrenceExtractionResult {
            occurrences: vec![OccurrenceCandidate {
                scientific_name: "Metaphire hilgendorfi".to_string(),
                locality: None,
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
