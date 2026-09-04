use std::{env, path::Path, time::Duration};

use axum::body::Bytes;
use futures_util::{StreamExt, stream};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE},
};
use tokio_util::io::ReaderStream;

use super::grobid::GrobidError;

pub struct GrobidFulltextClient {
    http: Client,
    base_url: String,
}

impl GrobidFulltextClient {
    pub fn from_env() -> Result<Self, GrobidError> {
        let base_url =
            env::var("GROBID_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8070".to_string());
        Self::from_base_url_with_timeout(&base_url, Duration::from_secs(120))
    }

    pub fn from_base_url_with_timeout(
        base_url: &str,
        timeout: Duration,
    ) -> Result<Self, GrobidError> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        let parsed_url =
            reqwest::Url::parse(&base_url).map_err(|_| GrobidError::InvalidConfiguration)?;
        if base_url.is_empty()
            || !matches!(parsed_url.scheme(), "http" | "https")
            || timeout.is_zero()
        {
            return Err(GrobidError::InvalidConfiguration);
        }

        let http = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| GrobidError::InvalidConfiguration)?;

        Ok(Self { http, base_url })
    }

    pub async fn extract_tei(
        &self,
        pdf_path: &Path,
        pdf_size_bytes: u64,
    ) -> Result<String, GrobidError> {
        let boundary = format!(
            "occurrence-web-grobid-fulltext-{}",
            uuid::Uuid::new_v4().simple()
        );
        let prefix = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"input\"; filename=\"paper.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
        );
        let suffix = format!(
            "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"consolidateHeader\"\r\n\r\n0\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"consolidateCitations\"\r\n\r\n0\r\n--{boundary}--\r\n"
        );

        let total_length = (prefix.len() as u64)
            .checked_add(pdf_size_bytes)
            .and_then(|value| value.checked_add(suffix.len() as u64))
            .ok_or(GrobidError::InvalidResponse)?;

        let file = tokio::fs::File::open(pdf_path)
            .await
            .map_err(GrobidError::FileSystem)?;
        let file_stream = ReaderStream::new(file);
        let prefix_stream =
            stream::once(async move { Ok::<Bytes, std::io::Error>(Bytes::from(prefix)) });
        let suffix_stream =
            stream::once(async move { Ok::<Bytes, std::io::Error>(Bytes::from(suffix)) });
        let body =
            reqwest::Body::wrap_stream(prefix_stream.chain(file_stream).chain(suffix_stream));

        let response = self
            .http
            .post(format!("{}/api/processFulltextDocument", self.base_url))
            .header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header(CONTENT_LENGTH, total_length)
            .header(ACCEPT, "application/xml")
            .body(body)
            .send()
            .await
            .map_err(|_| GrobidError::RequestFailed)?;

        if response.status() == StatusCode::NO_CONTENT {
            return Err(GrobidError::NoContent);
        }
        if !response.status().is_success() {
            return Err(GrobidError::Upstream(response.status()));
        }

        let tei = response
            .text()
            .await
            .map_err(|_| GrobidError::InvalidResponse)?;
        let trimmed = tei.trim();
        if trimmed.is_empty() || !(trimmed.contains("<TEI") || trimmed.contains(":TEI")) {
            return Err(GrobidError::InvalidResponse);
        }

        Ok(tei)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_configuration() {
        assert!(matches!(
            GrobidFulltextClient::from_base_url_with_timeout("://invalid", Duration::from_secs(1)),
            Err(GrobidError::InvalidConfiguration)
        ));
        assert!(matches!(
            GrobidFulltextClient::from_base_url_with_timeout(
                "http://127.0.0.1:8070",
                Duration::ZERO
            ),
            Err(GrobidError::InvalidConfiguration)
        ));
    }
}
