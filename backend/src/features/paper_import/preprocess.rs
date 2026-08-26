use std::{
    env,
    path::{Path, PathBuf},
};

use tempfile::TempDir;
use tokio::process::Command;

use super::{
    fulltext::GrobidFulltextClient,
    grobid::GrobidError,
};

pub const PAPER_PAGE_IMAGE_MEDIA_TYPE: &str = "image/jpeg";
pub const PAPER_PAGE_RENDER_DPI: u32 = 150;
const DEFAULT_PDFTOPPM_BIN: &str = "pdftoppm";

#[derive(Debug)]
pub enum PaperPreprocessError {
    InvalidInput,
    FileSystem(std::io::Error),
    Grobid(GrobidError),
    RendererUnavailable,
    RenderFailed(String),
    NoPageImages,
}

impl From<std::io::Error> for PaperPreprocessError {
    fn from(error: std::io::Error) -> Self {
        Self::FileSystem(error)
    }
}

impl From<GrobidError> for PaperPreprocessError {
    fn from(error: GrobidError) -> Self {
        Self::Grobid(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessedPageImage {
    pub page_number: u32,
    pub path: PathBuf,
    pub media_type: &'static str,
}

#[derive(Debug)]
pub struct PreprocessedPaper {
    /// Text extracted from the GROBID TEI front/body. References are excluded
    /// when GROBID returned the usual TEI structure.
    pub text: String,
    pub text_non_whitespace_chars: usize,
    /// Every PDF page is rendered so a vision model can recover information
    /// that is absent or structurally damaged in the extracted text.
    pub page_images: Vec<PreprocessedPageImage>,
    workspace: TempDir,
}

impl PreprocessedPaper {
    pub fn page_count(&self) -> usize {
        self.page_images.len()
    }

    /// The image paths remain valid while this value is alive.
    pub fn workspace_path(&self) -> &Path {
        self.workspace.path()
    }
}

pub struct PaperPdfPreprocessor;

impl PaperPdfPreprocessor {
    /// Convert one local PDF into the two inputs used by the multimodal LLM:
    /// GROBID-derived text and a JPEG image for every original page.
    pub async fn preprocess(pdf_path: &Path) -> Result<PreprocessedPaper, PaperPreprocessError> {
        let metadata = tokio::fs::metadata(pdf_path).await?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(PaperPreprocessError::InvalidInput);
        }

        let grobid = GrobidFulltextClient::from_env()?;
        let text = match grobid.extract_tei(pdf_path, metadata.len()).await {
            Ok(tei) => tei_to_llm_text(&tei),
            // A text layer is not guaranteed. In this case preprocessing still
            // succeeds because all pages are also supplied as images.
            Err(GrobidError::NoContent) => String::new(),
            Err(error) => return Err(PaperPreprocessError::Grobid(error)),
        };
        let text_non_whitespace_chars = text.chars().filter(|ch| !ch.is_whitespace()).count();

        let workspace = tempfile::Builder::new()
            .prefix("paper-llm-preprocess-")
            .tempdir()?;
        let page_images = render_all_pages(pdf_path, workspace.path()).await?;

        Ok(PreprocessedPaper {
            text,
            text_non_whitespace_chars,
            page_images,
            workspace,
        })
    }
}

async fn render_all_pages(
    pdf_path: &Path,
    output_dir: &Path,
) -> Result<Vec<PreprocessedPageImage>, PaperPreprocessError> {
    let renderer = env::var("PDFTOPPM_BIN").unwrap_or_else(|_| DEFAULT_PDFTOPPM_BIN.to_string());
    let renderer = renderer.trim();
    if renderer.is_empty() {
        return Err(PaperPreprocessError::RendererUnavailable);
    }

    let output_prefix = output_dir.join("page");
    let output = Command::new(renderer)
        .arg("-jpeg")
        .arg("-r")
        .arg(PAPER_PAGE_RENDER_DPI.to_string())
        .arg(pdf_path)
        .arg(&output_prefix)
        .output()
        .await;

    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(PaperPreprocessError::RendererUnavailable);
        }
        Err(error) => return Err(PaperPreprocessError::FileSystem(error)),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim().chars().take(1000).collect::<String>();
        return Err(PaperPreprocessError::RenderFailed(message));
    }

    let mut pages = Vec::new();
    let mut entries = tokio::fs::read_dir(output_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(page_number) = rendered_page_number(&path) else {
            continue;
        };
        pages.push(PreprocessedPageImage {
            page_number,
            path,
            media_type: PAPER_PAGE_IMAGE_MEDIA_TYPE,
        });
    }

    pages.sort_by_key(|page| page.page_number);
    if pages.is_empty() {
        return Err(PaperPreprocessError::NoPageImages);
    }

    Ok(pages)
}

fn rendered_page_number(path: &Path) -> Option<u32> {
    let extension = path.extension()?.to_str()?;
    if !extension.eq_ignore_ascii_case("jpg") && !extension.eq_ignore_ascii_case("jpeg") {
        return None;
    }

    let stem = path.file_stem()?.to_str()?;
    stem.strip_prefix("page-")?.parse().ok()
}

/// Build LLM text from GROBID TEI. The front and body are preferred so the
/// bibliography does not create false occurrence candidates from cited papers.
fn tei_to_llm_text(tei: &str) -> String {
    let mut selected = Vec::new();
    if let Some(front) = tei_section(tei, "front") {
        selected.push(front);
    }
    if let Some(body) = tei_section(tei, "body") {
        selected.push(body);
    }

    let source = if selected.is_empty() {
        tei.to_string()
    } else {
        selected.join("\n")
    };
    normalize_structured_text(&strip_xml_preserving_structure(&source))
}

fn tei_section<'a>(tei: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let start = tei.find(&open)?;
    let content_start = start + tei[start..].find('>')? + 1;
    let content_end = content_start + tei[content_start..].find(&close)?;
    Some(&tei[content_start..content_end])
}

fn strip_xml_preserving_structure(xml: &str) -> String {
    let mut output = String::with_capacity(xml.len());
    let mut chars = xml.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                let mut tag = String::new();
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                    tag.push(next);
                }
                append_tag_separator(&tag, &mut output);
            }
            '&' => {
                let mut entity = String::new();
                let mut terminated = false;
                for _ in 0..16 {
                    match chars.next() {
                        Some(';') => {
                            terminated = true;
                            break;
                        }
                        Some(next) => entity.push(next),
                        None => break,
                    }
                }

                if terminated {
                    if let Some(decoded) = decode_xml_entity(&entity) {
                        output.push(decoded);
                    } else {
                        output.push('&');
                        output.push_str(&entity);
                        output.push(';');
                    }
                } else {
                    output.push('&');
                    output.push_str(&entity);
                }
            }
            _ => output.push(ch),
        }
    }

    output
}

fn append_tag_separator(tag: &str, output: &mut String) {
    let trimmed = tag.trim();
    if trimmed.starts_with('?') || trimmed.starts_with('!') {
        return;
    }

    let trimmed = trimmed.trim_start_matches('/').trim();
    let token = trimmed.split_whitespace().next().unwrap_or("");
    let token = token.trim_end_matches('/');
    let name = token.rsplit(':').next().unwrap_or(token);

    match name {
        "cell" => output.push('\t'),
        "head" | "p" | "div" | "ab" | "list" | "item" | "figure" | "figDesc"
        | "table" | "row" | "note" | "quote" | "pb" | "lb" => output.push('\n'),
        _ => {}
    }
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ if entity.starts_with("#x") => {
            u32::from_str_radix(&entity[2..], 16).ok().and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn normalize_structured_text(raw: &str) -> String {
    let mut lines = Vec::new();

    for raw_line in raw.lines() {
        let columns = raw_line
            .split('\t')
            .map(collapse_whitespace)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !columns.is_empty() {
            lines.push(columns.join("\t"));
        }
    }

    lines.join("\n")
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_front_and_body_without_bibliography() {
        let tei = r#"<?xml version="1.0"?>
<TEI xmlns="http://www.tei-c.org/ns/1.0">
  <text>
    <front><div><p>Abstract &amp; title</p></div></front>
    <body>
      <div><head>Materials</head><p>Metaphire hilgendorfi was collected in Tokyo.</p></div>
      <table><row><cell>Species</cell><cell>Locality</cell></row><row><cell>M. hilgendorfi</cell><cell>Tokyo</cell></row></table>
    </body>
    <back><listBibl><biblStruct>Metaphire cited-only locality Osaka</biblStruct></listBibl></back>
  </text>
</TEI>"#;

        let text = tei_to_llm_text(tei);

        assert!(text.contains("Abstract & title"));
        assert!(text.contains("Metaphire hilgendorfi was collected in Tokyo."));
        assert!(text.contains("Species\tLocality"));
        assert!(!text.contains("cited-only"));
    }

    #[test]
    fn decodes_numeric_xml_entities() {
        let tei = "<TEI><text><body><p>35&#176; N &#x26; 139&#176; E</p></body></text></TEI>";
        assert_eq!(tei_to_llm_text(tei), "35° N & 139° E");
    }

    #[test]
    fn recognizes_pdftoppm_page_images() {
        assert_eq!(rendered_page_number(Path::new("/tmp/page-1.jpg")), Some(1));
        assert_eq!(rendered_page_number(Path::new("/tmp/page-023.jpeg")), Some(23));
        assert_eq!(rendered_page_number(Path::new("/tmp/other-1.jpg")), None);
        assert_eq!(rendered_page_number(Path::new("/tmp/page-1.png")), None);
    }
}
