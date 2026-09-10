use std::{
    env, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    routing::post,
};
use backend::features::paper_import::{
    llama::{LlamaClient, OCCURRENCE_EXTRACTION_PROMPT},
    preprocess::PaperPdfPreprocessor,
};
use serde_json::{Value, json};

const CAPTURE_REQUEST_BODY_LIMIT_BYTES: usize = 200 * 1024 * 1024;

#[derive(Clone, Default)]
struct CaptureState {
    request: Arc<Mutex<Option<Value>>>,
}

async fn capture_request(
    State(state): State<CaptureState>,
    Json(request): Json<Value>,
) -> Json<Value> {
    *state
        .request
        .lock()
        .expect("capture request lock should not be poisoned") = Some(request);

    // Return a valid empty result so the production LlamaClient finishes
    // parsing without requiring a real llama.cpp server.
    Json(json!({
        "choices": [{
            "message": {
                "content": "{\"occurrences\":[]}"
            }
        }]
    }))
}

fn debug_error(label: &str, error: impl std::fmt::Debug) -> io::Error {
    io::Error::other(format!("{label}: {error:?}"))
}

async fn copy_page_images(
    output_dir: &Path,
    pages: &[backend::features::paper_import::preprocess::PreprocessedPageImage],
) -> io::Result<()> {
    let images_dir = output_dir.join("images");
    tokio::fs::create_dir_all(&images_dir).await?;

    for page in pages {
        let destination = images_dir.join(format!("page-{}.jpg", page.page_number));
        tokio::fs::copy(&page.path, destination).await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let pdf_path = args.next().map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: cargo run --example paper_llama_dry_run -- <paper.pdf> [output-dir]",
        )
    })?;
    let output_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("paper-llama-dry-run"));

    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many arguments; expected <paper.pdf> [output-dir]",
        )
        .into());
    }

    println!("preprocessing PDF: {}", pdf_path.display());
    let paper = PaperPdfPreprocessor::preprocess(&pdf_path)
        .await
        .map_err(|error| debug_error("paper preprocessing failed", error))?;

    tokio::fs::create_dir_all(&output_dir).await?;
    tokio::fs::write(output_dir.join("prompt.txt"), OCCURRENCE_EXTRACTION_PROMPT).await?;
    tokio::fs::write(output_dir.join("extracted_text.txt"), &paper.text).await?;
    copy_page_images(&output_dir, &paper.page_images).await?;

    let capture = CaptureState::default();
    let captured_request = Arc::clone(&capture.request);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = Router::new()
        .route("/v1/chat/completions", post(capture_request))
        // Axum's default JSON body limit is too small for a multimodal request
        // containing all rendered pages as base64 data URIs. The real PDF
        // upload limit is 100 MiB, so 200 MiB is sufficient for this dry-run
        // capture after base64 expansion and JSON overhead.
        .layer(DefaultBodyLimit::max(CAPTURE_REQUEST_BODY_LIMIT_BYTES))
        .with_state(capture);
    let server = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("capture server failed: {error}");
        }
    });
    tokio::task::yield_now().await;

    let endpoint = format!("http://{address}/v1/chat/completions");
    let client = LlamaClient::new(&endpoint, "local-model", Duration::from_secs(30))
        .map_err(|error| debug_error("failed to create llama client", error))?;

    // This exercises the exact production request-building and HTTP JSON
    // serialization path, but sends only to the local capture server above.
    client
        .extract_occurrences_from_parts(&paper.text, &paper.page_images)
        .await
        .map_err(|error| debug_error("captured llama request failed", error))?;

    server.abort();

    let request = captured_request
        .lock()
        .expect("captured request lock should not be poisoned")
        .take()
        .ok_or_else(|| io::Error::other("no llama request was captured"))?;

    let pretty_request = serde_json::to_vec_pretty(&request)?;
    tokio::fs::write(output_dir.join("request.json"), pretty_request).await?;

    let request_bytes = serde_json::to_vec(&request)?.len();
    println!("dry run complete; no real llama server was contacted");
    println!(
        "text chars (non-whitespace): {}",
        paper.text_non_whitespace_chars
    );
    println!("page images: {}", paper.page_count());
    println!("captured request JSON bytes: {request_bytes}");
    println!("output: {}", output_dir.display());
    println!("  prompt.txt           fixed prompt");
    println!("  extracted_text.txt   GROBID-derived text sent to llama");
    println!("  images/page-N.jpg    page images sent to llama");
    println!("  request.json         exact captured multimodal HTTP JSON body");
    println!("request.json contains the same images as base64 data:image/jpeg URIs");

    Ok(())
}
