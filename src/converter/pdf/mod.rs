pub mod assembler;
pub mod classifier;
pub mod extractor;

use crate::converter::Converter;
use crate::error::ConvertError;
use crate::model::document::{Document, Metadata};
use crate::model::options::ConvertOptions;
use assembler::Assembler;
use classifier::Classifier;
use extractor::PdfExtractor;
use std::path::Path;
use tracing::{debug, info};

pub struct PdfConverter;

impl Converter for PdfConverter {
    fn name(&self) -> &str {
        "pdf"
    }

    fn supported_extensions(&self) -> &[&str] {
        &["pdf"]
    }

    fn convert(&self, input: &Path, _options: &ConvertOptions) -> Result<Document, ConvertError> {
        info!(input = %input.display(), "PDF conversion started");

        debug!("Phase 1: Extracting raw elements");
        let raw_pages = PdfExtractor::extract(input)?;
        debug!(pages = raw_pages.len(), "Extraction complete");

        debug!("Phase 2: Classifying elements");
        let classified = Classifier::classify(&raw_pages);

        debug!("Phase 3: Extracting metadata");
        let pdf_meta = PdfExtractor::extract_metadata(input);
        let metadata = Metadata {
            title: pdf_meta.title,
            author: pdf_meta.author,
            date: pdf_meta.date,
        };

        debug!("Phase 4: Assembling document");
        let doc = Assembler::assemble(classified, metadata);
        info!(
            pages = doc.pages.len(),
            elements = doc.pages.iter().map(|p| p.elements.len()).sum::<usize>(),
            "PDF conversion complete"
        );
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_converter_name() {
        let conv = PdfConverter;
        assert_eq!(conv.name(), "pdf");
    }

    #[test]
    fn test_pdf_converter_extensions() {
        let conv = PdfConverter;
        assert_eq!(conv.supported_extensions(), &["pdf"]);
    }

    #[test]
    fn test_pdf_converter_file_not_found() {
        let conv = PdfConverter;
        let opts = ConvertOptions::default();
        let result = conv.convert(Path::new("nonexistent.pdf"), &opts);
        assert!(result.is_err());
    }
}
