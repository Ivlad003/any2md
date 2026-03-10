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

pub struct PdfConverter;

impl Converter for PdfConverter {
    fn name(&self) -> &str {
        "pdf"
    }

    fn supported_extensions(&self) -> &[&str] {
        &["pdf"]
    }

    fn convert(&self, input: &Path, _options: &ConvertOptions) -> Result<Document, ConvertError> {
        let raw_pages = PdfExtractor::extract(input)?;
        let classified = Classifier::classify(&raw_pages);
        let metadata = Metadata {
            title: None,
            author: None,
            date: None,
        };
        Ok(Assembler::assemble(classified, metadata))
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
