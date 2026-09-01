use std::path::Path;

use super::grobid::{GrobidClient, GrobidError, GrobidPaperMetadata, PaperMetadataExtractor};

impl GrobidClient {
    pub async fn extract_header(
        &self,
        pdf_path: &Path,
        pdf_size_bytes: u64,
    ) -> Result<GrobidPaperMetadata, GrobidError> {
        <Self as PaperMetadataExtractor>::extract_header(self, pdf_path, pdf_size_bytes).await
    }
}
