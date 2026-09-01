use std::{collections::HashMap, env, path::Path, time::Duration};

use axum::body::Bytes;
use futures_util::{StreamExt, stream};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE},
};
use tokio_util::io::ReaderStream;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrobidPaperMetadata {
    pub doi: Option<String>,
    pub title: Option<String>,
    pub authors: Option<String>,
    pub publication_year: Option<i32>,
    pub journal: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub article_number: Option<String>,
}

#[derive(Debug)]
pub enum GrobidError {
    FileSystem(std::io::Error),
    InvalidConfiguration,
    RequestFailed,
    NoContent,
    Upstream(StatusCode),
    InvalidResponse,
}

#[async_trait::async_trait]
pub trait PaperMetadataExtractor: Send + Sync {
    async fn extract_header(
        &self,
        pdf_path: &Path,
        pdf_size_bytes: u64,
    ) -> Result<GrobidPaperMetadata, GrobidError>;
}

pub struct GrobidClient {
    http: Client,
    base_url: String,
}

impl GrobidClient {
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

        // Keep one timeout for connection, upload, and response parsing. GROBID
        // processing can be slow, so production uses 120 seconds while tests
        // inject a short duration to exercise timeout handling deterministically.
        let http = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| GrobidError::InvalidConfiguration)?;

        Ok(Self { http, base_url })
    }
}

#[async_trait::async_trait]
impl PaperMetadataExtractor for GrobidClient {
    async fn extract_header(
        &self,
        pdf_path: &Path,
        pdf_size_bytes: u64,
    ) -> Result<GrobidPaperMetadata, GrobidError> {
        let boundary = format!("occurrence-web-grobid-{}", uuid::Uuid::new_v4().simple());
        let prefix = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"input\"; filename=\"paper.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
        );
        let suffix = format!(
            "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"consolidateHeader\"\r\n\r\n0\r\n--{boundary}--\r\n"
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
        let body_stream = prefix_stream.chain(file_stream).chain(suffix_stream);
        let body = reqwest::Body::wrap_stream(body_stream);

        let url = format!("{}/api/processHeaderDocument", self.base_url);
        let response = self
            .http
            .post(url)
            .header(
                CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .header(CONTENT_LENGTH, total_length)
            .header(ACCEPT, "application/x-bibtex")
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

        let bibtex = response
            .text()
            .await
            .map_err(|_| GrobidError::InvalidResponse)?;
        parse_grobid_bibtex(&bibtex)
    }
}

fn parse_grobid_bibtex(input: &str) -> Result<GrobidPaperMetadata, GrobidError> {
    let fields = parse_bibtex_fields(input).ok_or(GrobidError::InvalidResponse)?;

    let doi = field(&fields, &["doi"]).map(normalize_doi);
    let title = field(&fields, &["title"]);
    let authors = field(&fields, &["author"]).map(normalize_authors);
    let publication_year = field(&fields, &["year"]).and_then(|value| extract_year(&value));
    let journal = field(&fields, &["journal"]);
    let volume = field(&fields, &["volume"]);
    let issue = field(&fields, &["number", "issue"]);
    let pages = field(&fields, &["pages"]).map(normalize_pages);
    let article_number = field(
        &fields,
        &["eid", "article_number", "article-number", "articlenumber"],
    );

    Ok(GrobidPaperMetadata {
        doi,
        title,
        authors,
        publication_year,
        journal,
        volume,
        issue,
        pages,
        article_number,
    })
}

fn field(fields: &HashMap<String, String>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| fields.get(*name))
        .map(|value| clean_bibtex_value(value))
        .filter(|value| !value.is_empty())
}

fn parse_bibtex_fields(input: &str) -> Option<HashMap<String, String>> {
    // GROBID promises a BibTeX entry, not merely arbitrary text containing
    // braces. Requiring the entry marker prevents malformed 200 responses from
    // being accepted as empty metadata.
    if !input.trim_start().starts_with('@') {
        return None;
    }

    let bytes = input.as_bytes();
    let entry_open = bytes.iter().position(|byte| *byte == b'{')?;

    let mut index = entry_open + 1;
    let mut depth = 0_i32;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b',' if depth == 0 => {
                index += 1;
                break;
            }
            _ => {}
        }
        index += 1;
    }

    if index >= bytes.len() {
        return None;
    }

    let mut fields = HashMap::new();
    let mut entry_closed = false;

    while index < bytes.len() {
        skip_separators(bytes, &mut index);
        if index >= bytes.len() {
            break;
        }
        if bytes[index] == b'}' {
            entry_closed = true;
            break;
        }

        let key_start = index;
        while index < bytes.len() && bytes[index] != b'=' && bytes[index] != b'}' {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            break;
        }
        let key = input[key_start..index].trim().to_ascii_lowercase();
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        let value = match bytes.get(index).copied() {
            Some(b'{') => parse_braced_value(input, bytes, &mut index)?,
            Some(b'\"') => parse_quoted_value(input, bytes, &mut index)?,
            Some(_) => parse_bare_value(input, bytes, &mut index),
            None => return None,
        };

        if !key.is_empty() {
            fields.insert(key, value);
        }
    }

    entry_closed.then_some(fields)
}

fn skip_separators(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && (bytes[*index].is_ascii_whitespace() || bytes[*index] == b',') {
        *index += 1;
    }
}

fn parse_braced_value(input: &str, bytes: &[u8], index: &mut usize) -> Option<String> {
    *index += 1;
    let start = *index;
    let mut depth = 1_i32;

    while *index < bytes.len() {
        match bytes[*index] {
            b'\\' => {
                *index = (*index).saturating_add(2);
                continue;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let value = input[start..*index].to_string();
                    *index += 1;
                    return Some(value);
                }
            }
            _ => {}
        }
        *index += 1;
    }

    None
}

fn parse_quoted_value(input: &str, bytes: &[u8], index: &mut usize) -> Option<String> {
    *index += 1;
    let start = *index;

    while *index < bytes.len() {
        if bytes[*index] == b'\\' {
            *index = (*index).saturating_add(2);
            continue;
        }
        if bytes[*index] == b'\"' {
            let value = input[start..*index].to_string();
            *index += 1;
            return Some(value);
        }
        *index += 1;
    }

    None
}

fn parse_bare_value(input: &str, bytes: &[u8], index: &mut usize) -> String {
    let start = *index;
    while *index < bytes.len() && bytes[*index] != b',' && bytes[*index] != b'}' {
        *index += 1;
    }
    input[start..*index].trim().to_string()
}

fn clean_bibtex_value(value: &str) -> String {
    let mut cleaned = value
        .replace("\\&", "&")
        .replace("\\%", "%")
        .replace("\\_", "_")
        .replace("~", " ")
        .replace(['\n', '\r', '\t'], " ");

    cleaned = cleaned.replace(['{', '}'], "");
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn normalize_authors(value: String) -> String {
    value
        .split(" and ")
        .map(str::trim)
        .filter(|author| !author.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn normalize_doi(value: String) -> String {
    let value = value.trim();
    let value = value
        .strip_prefix("https://doi.org/")
        .or_else(|| value.strip_prefix("http://doi.org/"))
        .or_else(|| value.strip_prefix("doi:"))
        .unwrap_or(value);
    value.trim().to_string()
}

fn normalize_pages(value: String) -> String {
    value.replace("--", "-")
}

fn extract_year(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    for window in bytes.windows(4) {
        if window.iter().all(u8::is_ascii_digit) {
            let year = std::str::from_utf8(window).ok()?.parse::<i32>().ok()?;
            if (1000..=3000).contains(&year) {
                return Some(year);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grobid_bibtex_header_fields() {
        let bibtex = r#"@article{sample,
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

        let metadata = parse_grobid_bibtex(bibtex).expect("BibTeX should parse");

        assert_eq!(metadata.doi.as_deref(), Some("10.1234/example.1"));
        assert_eq!(
            metadata.title.as_deref(),
            Some("A study of DNA in earthworms")
        );
        assert_eq!(
            metadata.authors.as_deref(),
            Some("Doe, Jane; Smith, John Q.")
        );
        assert_eq!(metadata.publication_year, Some(2025));
        assert_eq!(metadata.journal.as_deref(), Some("Example Journal"));
        assert_eq!(metadata.volume.as_deref(), Some("12"));
        assert_eq!(metadata.issue.as_deref(), Some("3"));
        assert_eq!(metadata.pages.as_deref(), Some("101-115"));
        assert_eq!(metadata.article_number.as_deref(), Some("e12345"));
    }

    #[test]
    fn missing_optional_fields_remain_none() {
        let bibtex = r#"@article{sample,
  title = {Only a title}
}"#;

        let metadata = parse_grobid_bibtex(bibtex).expect("BibTeX should parse");

        assert_eq!(metadata.title.as_deref(), Some("Only a title"));
        assert_eq!(metadata.doi, None);
        assert_eq!(metadata.authors, None);
        assert_eq!(metadata.publication_year, None);
        assert_eq!(metadata.journal, None);
        assert_eq!(metadata.volume, None);
        assert_eq!(metadata.issue, None);
        assert_eq!(metadata.pages, None);
        assert_eq!(metadata.article_number, None);
    }

    #[test]
    fn normalizes_doi_authors_and_pages() {
        let bibtex = r#"@article{sample,
  author = {Doe, Jane and Smith, John},
  pages = {10--20},
  doi = {doi:10.9999/test}
}"#;

        let metadata = parse_grobid_bibtex(bibtex).expect("BibTeX should parse");

        assert_eq!(metadata.authors.as_deref(), Some("Doe, Jane; Smith, John"));
        assert_eq!(metadata.pages.as_deref(), Some("10-20"));
        assert_eq!(metadata.doi.as_deref(), Some("10.9999/test"));
    }

    #[test]
    fn does_not_guess_article_number_from_pages() {
        let bibtex = r#"@article{sample,
  title = {Example},
  pages = {055406}
}"#;

        let metadata = parse_grobid_bibtex(bibtex).expect("BibTeX should parse");

        assert_eq!(metadata.pages.as_deref(), Some("055406"));
        assert_eq!(metadata.article_number, None);
    }

    #[test]
    fn parses_valid_bibtex_with_no_metadata_fields() {
        let metadata =
            parse_grobid_bibtex("@article{sample,\n}").expect("empty BibTeX entry should parse");

        assert_eq!(metadata, GrobidPaperMetadata::default());
    }

    #[test]
    fn rejects_truncated_bibtex() {
        let result = parse_grobid_bibtex("@article{sample, title={Incomplete}");

        assert!(matches!(result, Err(GrobidError::InvalidResponse)));
    }

    #[test]
    fn rejects_malformed_bibtex() {
        let result = parse_grobid_bibtex("not bibtex");
        assert!(matches!(result, Err(GrobidError::InvalidResponse)));
    }
}
