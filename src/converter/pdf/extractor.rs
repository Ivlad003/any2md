use crate::error::ConvertError;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RawTextBlock {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub font_size: f64,
    pub font_name: String,
}

#[derive(Debug, Clone)]
pub struct RawImage {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub enum RawElement {
    Text(RawTextBlock),
    Image(RawImage),
}

pub struct RawPage {
    pub elements: Vec<RawElement>,
}

pub struct PdfExtractor;

impl PdfExtractor {
    pub fn extract(path: &Path) -> Result<Vec<RawPage>, ConvertError> {
        if !path.exists() {
            return Err(ConvertError::FileNotFound(path.to_path_buf()));
        }

        let doc = lopdf::Document::load(path)
            .map_err(|e| ConvertError::CorruptedFile(format!("Failed to parse PDF: {}", e)))?;

        let mut pages = Vec::new();
        let page_count = doc.get_pages().len();

        for page_num in 1..=page_count as u32 {
            let raw_page = Self::extract_page(&doc, page_num)?;
            pages.push(raw_page);
        }

        Ok(pages)
    }

    fn extract_page(doc: &lopdf::Document, page_num: u32) -> Result<RawPage, ConvertError> {
        let mut elements = Vec::new();

        if let Ok(content) = doc.extract_text(&[page_num]) {
            let lines: Vec<&str> = content.lines().collect();
            let mut y_pos = 800.0;
            for line in lines {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    elements.push(RawElement::Text(RawTextBlock {
                        text: trimmed.to_string(),
                        x: 72.0,
                        y: y_pos,
                        font_size: 12.0,
                        font_name: "Unknown".to_string(),
                    }));
                    y_pos -= 14.0;
                }
            }
        }

        Ok(RawPage { elements })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_text_block_creation() {
        let block = RawTextBlock {
            text: "Hello".to_string(),
            x: 72.0,
            y: 700.0,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
        };
        assert_eq!(block.text, "Hello");
        assert_eq!(block.font_size, 12.0);
    }

    #[test]
    fn test_raw_image_creation() {
        let img = RawImage {
            data: vec![0x89, 0x50],
            width: 100,
            height: 50,
        };
        assert_eq!(img.width, 100);
        assert_eq!(img.data.len(), 2);
    }

    #[test]
    fn test_extract_nonexistent_file() {
        let result = PdfExtractor::extract(Path::new("nonexistent.pdf"));
        assert!(result.is_err());
    }
}
