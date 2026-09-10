use std::path::Path;

use super::llama::{
    OccurrenceExtractionResult, PaperLlmExtractionError, extract_occurrences_from_pdf,
};

#[async_trait::async_trait]
pub trait PaperOccurrenceExtractor: Send + Sync {
    async fn extract(
        &self,
        pdf_path: &Path,
    ) -> Result<OccurrenceExtractionResult, PaperLlmExtractionError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LlamaPaperOccurrenceExtractor;

#[async_trait::async_trait]
impl PaperOccurrenceExtractor for LlamaPaperOccurrenceExtractor {
    async fn extract(
        &self,
        pdf_path: &Path,
    ) -> Result<OccurrenceExtractionResult, PaperLlmExtractionError> {
        extract_occurrences_from_pdf(pdf_path).await
    }
}
