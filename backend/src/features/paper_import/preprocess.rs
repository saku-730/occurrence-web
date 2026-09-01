use std::{
    env,
    path::{Path, PathBuf},
};

use tempfile::TempDir;
use tokio::process::Command;

use super::{fulltext::GrobidFulltextClient, grobid::GrobidError};

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
    /// Text extracted from the GROBID TEI front/body when available.
    /// GROBID failures are non-fatal; in that case this is empty and the LLM
    /// receives rendered page images only.
    pub text: String,
    pub text_non_whitespace_chars: usize,
    /// Every PDF page is rendered so a vision model can recover information
    /// when text extraction is unavailable or incomplete.
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
    /// Convert one local PDF into the multimodal inputs used by the LLM.
    /// GROBID fulltext extraction is best effort. Any GROBID failure falls back
    /// to image-only extraction as long as every PDF page can still be rendered.
    pub async fn preprocess(pdf_path: &Path) -> Result<PreprocessedPaper, PaperPreprocessError> {
        let metadata = tokio::fs::metadata(pdf_path).await?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(PaperPreprocessError::InvalidInput);
        }

        let text = match GrobidFulltextClient::from_env() {
            Ok(grobid) => match grobid.extract_tei(pdf_path, metadata.len()).await {
                Ok(tei) => tei_to_llm_text(&tei),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
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

    // Do not fall back to the complete TEI document. A TEI containing only a
    // bibliography would otherwise feed cited taxa and locations into the
    // occurrence extractor. Page images remain available for such PDFs.
    normalize_structured_text(&strip_xml_preserving_structure(&selected.join("\n")))
}

fn tei_section<'a>(tei: &'a str, name: &str) -> Option<&'a str> {
    let (content_start, _) = find_tei_tag(tei, name, false, 0)?;
    let (content_end, _) = find_tei_tag(tei, name, true, content_start)?;
    Some(&tei[content_start..content_end])
}

/// GROBID normally uses the default TEI namespace, but valid TEI can also use
/// a prefix such as `tei:body`. Match the local name so both forms preserve
/// the front/body-only extraction rule.
fn find_tei_tag(
    xml: &str,
    expected_name: &str,
    closing: bool,
    from: usize,
) -> Option<(usize, usize)> {
    let mut cursor = from;

    while let Some(relative_start) = xml[cursor..].find('<') {
        let start = cursor + relative_start;
        let tag_end = start + xml[start..].find('>')?;
        let tag = &xml[start + 1..tag_end];
        let tag = tag.trim();
        let is_closing = tag.starts_with('/');
        let token = tag
            .trim_start_matches('/')
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        let local_name = token.rsplit(':').next().unwrap_or(token);

        if is_closing == closing && local_name == expected_name {
            return Some((start, tag_end + 1));
        }

        cursor = tag_end + 1;
    }

    None
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
        "head" | "p" | "div" | "ab" | "list" | "item" | "figure" | "figDesc" | "table" | "row"
        | "note" | "quote" | "pb" | "lb" => output.push('\n'),
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
        _ if entity.starts_with("#x") => u32::from_str_radix(&entity[2..], 16)
            .ok()
            .and_then(char::from_u32),
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
    use std::{
        os::unix::fs::PermissionsExt,
        sync::{Mutex, OnceLock},
    };

    use axum::{Router, body::Bytes, http::StatusCode, response::IntoResponse, routing::post};

    use super::*;

    static ENVIRONMENT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn environment_lock() -> std::sync::MutexGuard<'static, ()> {
        ENVIRONMENT_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write_fake_renderer(script: &str) -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("fake renderer directory should be created");
        let path = directory.path().join("pdftoppm");
        std::fs::write(&path, script).expect("fake renderer should be written");
        let mut permissions = std::fs::metadata(&path)
            .expect("fake renderer metadata should be readable")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("fake renderer should be executable");
        directory
    }

    async fn start_grobid(
        status: StatusCode,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        async fn handler(
            axum::extract::State((status, body)): axum::extract::State<(StatusCode, &'static str)>,
            _body: Bytes,
        ) -> impl IntoResponse {
            (status, body)
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock GROBID listener should bind");
        let address = listener
            .local_addr()
            .expect("mock GROBID address should exist");
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/api/processFulltextDocument", post(handler))
                    .with_state((status, body)),
            )
            .await
            .expect("mock GROBID server should run");
        });

        (format!("http://{address}"), server)
    }

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
    fn extracts_namespaced_front_and_body_without_bibliography() {
        let tei = r#"<tei:TEI xmlns:tei="http://www.tei-c.org/ns/1.0">
  <tei:text>
    <tei:front><tei:p>Article title</tei:p></tei:front>
    <tei:body><tei:p>Observed in Kyoto.</tei:p></tei:body>
    <tei:back><tei:p>Cited-only occurrence in Osaka.</tei:p></tei:back>
  </tei:text>
</tei:TEI>"#;

        let text = tei_to_llm_text(tei);

        assert!(text.contains("Article title"));
        assert!(text.contains("Observed in Kyoto."));
        assert!(!text.contains("Cited-only"));
    }

    #[test]
    fn does_not_fall_back_to_bibliography_when_front_and_body_are_missing() {
        let tei = r#"<TEI><text><back><p>Cited-only occurrence in Osaka.</p></back></text></TEI>"#;

        assert!(tei_to_llm_text(tei).is_empty());
    }

    #[test]
    fn decodes_numeric_xml_entities() {
        let tei = "<TEI><text><body><p>35&#176; N &#x26; 139&#176; E</p></body></text></TEI>";
        assert_eq!(tei_to_llm_text(tei), "35° N & 139° E");
    }

    #[test]
    fn recognizes_pdftoppm_page_images() {
        assert_eq!(rendered_page_number(Path::new("/tmp/page-1.jpg")), Some(1));
        assert_eq!(
            rendered_page_number(Path::new("/tmp/page-023.jpeg")),
            Some(23)
        );
        assert_eq!(rendered_page_number(Path::new("/tmp/other-1.jpg")), None);
        assert_eq!(rendered_page_number(Path::new("/tmp/page-1.png")), None);
    }

    #[tokio::test]
    async fn preprocess_keeps_tei_text_and_sorts_all_rendered_page_images() {
        let _guard = environment_lock();
        let (grobid_base_url, server) = start_grobid(
            StatusCode::OK,
            "<TEI><text><front><p>Paper title</p></front><body><p>Observed in Kyoto.</p></body><back><p>Cited-only</p></back></text></TEI>",
        )
        .await;
        let renderer = write_fake_renderer(
            "#!/bin/sh\n\
             prefix=\"$5\"\n\
             : > \"$prefix-10.jpg\"\n\
             : > \"$prefix-2.jpeg\"\n\
             : > \"$prefix-1.jpg\"\n\
             : > \"$prefix-not-a-page.png\"\n",
        );
        let pdf = tempfile::NamedTempFile::with_suffix(".pdf").expect("test PDF should be created");
        std::fs::write(pdf.path(), b"%PDF-1.7\nmock").expect("test PDF should be written");

        let old_grobid_base_url = env::var_os("GROBID_BASE_URL");
        let old_renderer = env::var_os("PDFTOPPM_BIN");
        unsafe {
            env::set_var("GROBID_BASE_URL", grobid_base_url);
            env::set_var("PDFTOPPM_BIN", renderer.path().join("pdftoppm"));
        }
        let output = PaperPdfPreprocessor::preprocess(pdf.path()).await;
        match old_grobid_base_url {
            Some(value) => unsafe { env::set_var("GROBID_BASE_URL", value) },
            None => unsafe { env::remove_var("GROBID_BASE_URL") },
        }
        match old_renderer {
            Some(value) => unsafe { env::set_var("PDFTOPPM_BIN", value) },
            None => unsafe { env::remove_var("PDFTOPPM_BIN") },
        }
        server.abort();

        let output = output.expect("text and rendered pages should be returned");
        assert!(output.text.contains("Paper title"));
        assert!(output.text.contains("Observed in Kyoto."));
        assert!(!output.text.contains("Cited-only"));
        assert_eq!(output.text_non_whitespace_chars, 26);
        assert_eq!(output.page_count(), 3);
        assert_eq!(
            output
                .page_images
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2, 10]
        );
        assert!(
            output
                .page_images
                .iter()
                .all(|page| page.media_type == PAPER_PAGE_IMAGE_MEDIA_TYPE)
        );
        let workspace_path = output.workspace_path().to_path_buf();
        assert!(workspace_path.is_dir());
        drop(output);
        assert!(!workspace_path.exists());
    }

    #[tokio::test]
    async fn preprocess_rejects_renderer_failure_and_empty_output() {
        let _guard = environment_lock();
        let pdf = tempfile::NamedTempFile::with_suffix(".pdf").expect("test PDF should be created");
        std::fs::write(pdf.path(), b"%PDF-1.7\nmock").expect("test PDF should be written");
        let old_renderer = env::var_os("PDFTOPPM_BIN");

        let failing_renderer =
            write_fake_renderer("#!/bin/sh\necho rendering failed >&2\nexit 4\n");
        unsafe { env::set_var("PDFTOPPM_BIN", failing_renderer.path().join("pdftoppm")) };
        let failed = render_all_pages(
            pdf.path(),
            tempfile::tempdir()
                .expect("render output directory should be created")
                .path(),
        )
        .await;
        assert!(
            matches!(failed, Err(PaperPreprocessError::RenderFailed(message)) if message == "rendering failed")
        );

        let empty_renderer = write_fake_renderer("#!/bin/sh\nexit 0\n");
        unsafe { env::set_var("PDFTOPPM_BIN", empty_renderer.path().join("pdftoppm")) };
        let empty = render_all_pages(
            pdf.path(),
            tempfile::tempdir()
                .expect("render output directory should be created")
                .path(),
        )
        .await;
        match old_renderer {
            Some(value) => unsafe { env::set_var("PDFTOPPM_BIN", value) },
            None => unsafe { env::remove_var("PDFTOPPM_BIN") },
        }

        assert!(matches!(empty, Err(PaperPreprocessError::NoPageImages)));
    }

    #[tokio::test]
    async fn preprocess_continues_with_page_images_when_grobid_has_no_content() {
        let _guard = environment_lock();
        let (grobid_base_url, server) = start_grobid(StatusCode::NO_CONTENT, "").await;
        let renderer = write_fake_renderer(
            "#!/bin/sh\n\
             prefix=\"$5\"\n\
             : > \"$prefix-2.jpg\"\n\
             : > \"$prefix-1.jpg\"\n",
        );
        let pdf = tempfile::NamedTempFile::with_suffix(".pdf").expect("test PDF should be created");
        std::fs::write(pdf.path(), b"%PDF-1.7\nmock").expect("test PDF should be written");

        let old_grobid_base_url = env::var_os("GROBID_BASE_URL");
        let old_renderer = env::var_os("PDFTOPPM_BIN");
        unsafe {
            env::set_var("GROBID_BASE_URL", grobid_base_url);
            env::set_var("PDFTOPPM_BIN", renderer.path().join("pdftoppm"));
        }
        let output = PaperPdfPreprocessor::preprocess(pdf.path()).await;
        match old_grobid_base_url {
            Some(value) => unsafe { env::set_var("GROBID_BASE_URL", value) },
            None => unsafe { env::remove_var("GROBID_BASE_URL") },
        }
        match old_renderer {
            Some(value) => unsafe { env::set_var("PDFTOPPM_BIN", value) },
            None => unsafe { env::remove_var("PDFTOPPM_BIN") },
        }
        server.abort();

        let output = output.expect("image-only preprocessing should succeed");
        assert!(output.text.is_empty());
        assert_eq!(output.text_non_whitespace_chars, 0);
        assert_eq!(output.page_count(), 2);
        assert_eq!(
            output
                .page_images
                .iter()
                .map(|page| page.page_number)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(output.page_images.iter().all(|page| page.path.is_file()));
    }

    #[tokio::test]
    async fn preprocess_continues_with_page_images_when_grobid_returns_error() {
        let _guard = environment_lock();
        let (grobid_base_url, server) =
            start_grobid(StatusCode::SERVICE_UNAVAILABLE, "GROBID unavailable").await;
        let renderer = write_fake_renderer(
            "#!/bin/sh\n\
             prefix=\"$5\"\n\
             : > \"$prefix-1.jpg\"\n",
        );
        let pdf = tempfile::NamedTempFile::with_suffix(".pdf").expect("test PDF should be created");
        std::fs::write(pdf.path(), b"%PDF-1.7\nmock").expect("test PDF should be written");

        let old_grobid_base_url = env::var_os("GROBID_BASE_URL");
        let old_renderer = env::var_os("PDFTOPPM_BIN");
        unsafe {
            env::set_var("GROBID_BASE_URL", grobid_base_url);
            env::set_var("PDFTOPPM_BIN", renderer.path().join("pdftoppm"));
        }
        let output = PaperPdfPreprocessor::preprocess(pdf.path()).await;
        match old_grobid_base_url {
            Some(value) => unsafe { env::set_var("GROBID_BASE_URL", value) },
            None => unsafe { env::remove_var("GROBID_BASE_URL") },
        }
        match old_renderer {
            Some(value) => unsafe { env::set_var("PDFTOPPM_BIN", value) },
            None => unsafe { env::remove_var("PDFTOPPM_BIN") },
        }
        server.abort();

        let output = output.expect("GROBID failure must fall back to image-only preprocessing");
        assert!(output.text.is_empty());
        assert_eq!(output.text_non_whitespace_chars, 0);
        assert_eq!(output.page_count(), 1);
    }
}
