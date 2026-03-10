use crate::error::ConvertError;
use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeMap;
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

/// PDF document metadata extracted from the Info dictionary.
pub struct PdfMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
}

pub struct PdfExtractor;

impl PdfExtractor {
    pub fn extract(path: &Path) -> Result<Vec<RawPage>, ConvertError> {
        if !path.exists() {
            return Err(ConvertError::FileNotFound(path.to_path_buf()));
        }

        let doc = Document::load(path)
            .map_err(|e| ConvertError::CorruptedFile(format!("Failed to parse PDF: {}", e)))?;

        let mut pages = Vec::new();
        let page_count = doc.get_pages().len();

        for page_num in 1..=page_count as u32 {
            let raw_page = Self::extract_page(&doc, page_num)?;
            pages.push(raw_page);
        }

        Ok(pages)
    }

    /// Extract metadata (title, author, date) from the PDF document info dictionary.
    pub fn extract_metadata(path: &Path) -> PdfMetadata {
        let doc = match Document::load(path) {
            Ok(d) => d,
            Err(_) => {
                return PdfMetadata {
                    title: None,
                    author: None,
                    date: None,
                }
            }
        };

        let info_dict = Self::get_info_dict(&doc);

        let title = info_dict
            .as_ref()
            .and_then(|d| Self::get_info_string(d, b"Title"));
        let author = info_dict
            .as_ref()
            .and_then(|d| Self::get_info_string(d, b"Author"));
        let date = info_dict
            .as_ref()
            .and_then(|d| Self::get_info_string(d, b"CreationDate"))
            .map(|s| Self::parse_pdf_date(&s));

        PdfMetadata {
            title,
            author,
            date,
        }
    }

    /// Get the Info dictionary from the document trailer.
    fn get_info_dict(doc: &Document) -> Option<lopdf::Dictionary> {
        let info_obj = doc.trailer.get(b"Info").ok()?;
        match info_obj {
            Object::Dictionary(dict) => Some(dict.clone()),
            Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| {
                if let Object::Dictionary(dict) = o {
                    Some(dict.clone())
                } else {
                    None
                }
            }),
            _ => None,
        }
    }

    /// Extract a string value from an info dictionary by key.
    fn get_info_string(dict: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
        let obj = dict.get(key).ok()?;
        match obj {
            Object::String(bytes, _) => {
                // Try UTF-16BE first (starts with BOM 0xFE 0xFF)
                if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
                    let chars: Vec<u16> = bytes[2..]
                        .chunks(2)
                        .filter_map(|c| {
                            if c.len() == 2 {
                                Some(u16::from_be_bytes([c[0], c[1]]))
                            } else {
                                None
                            }
                        })
                        .collect();
                    String::from_utf16(&chars).ok()
                } else {
                    Some(String::from_utf8_lossy(bytes).into_owned())
                }
            }
            _ => None,
        }
    }

    /// Parse a PDF date string (D:YYYYMMDDHHmmSS) into a simpler format.
    fn parse_pdf_date(raw: &str) -> String {
        let s = raw.strip_prefix("D:").unwrap_or(raw);
        if s.len() >= 8 {
            let year = &s[0..4];
            let month = &s[4..6];
            let day = &s[6..8];
            format!("{}-{}-{}", year, month, day)
        } else {
            raw.to_string()
        }
    }

    fn extract_page(doc: &Document, page_num: u32) -> Result<RawPage, ConvertError> {
        let pages = doc.get_pages();
        let page_id = match pages.get(&page_num) {
            Some(id) => *id,
            None => return Ok(RawPage { elements: vec![] }),
        };

        // Try content-stream parsing first; fall back to extract_text on failure.
        match Self::extract_page_from_streams(doc, page_id) {
            Ok(page) if !page.elements.is_empty() => Ok(page),
            _ => Self::extract_page_fallback(doc, page_num),
        }
    }

    /// Parse PDF content streams to extract text with real font metadata.
    fn extract_page_from_streams(
        doc: &Document,
        page_id: ObjectId,
    ) -> Result<RawPage, ConvertError> {
        let content_data = doc
            .get_page_content(page_id)
            .map_err(|e| ConvertError::CorruptedFile(format!("Content stream error: {}", e)))?;

        let content = lopdf::content::Content::decode(&content_data)
            .map_err(|e| ConvertError::CorruptedFile(format!("Content decode error: {}", e)))?;

        // Build font name lookup: /F1 → "Helvetica-Bold", etc.
        let font_map = Self::build_font_map(doc, page_id);

        let mut elements = Vec::new();
        let mut current_font_tag = String::new();
        let mut current_font_size: f64 = 12.0;
        let mut x: f64 = 0.0;
        let mut y: f64 = 0.0;
        // Track text matrix position separately for Tm
        let mut tm_x: f64 = 0.0;
        let mut tm_y: f64 = 0.0;
        let mut in_text = false;

        for op in &content.operations {
            match op.operator.as_str() {
                "BT" => {
                    in_text = true;
                    // Reset text position at BT
                    tm_x = 0.0;
                    tm_y = 0.0;
                    x = 0.0;
                    y = 0.0;
                }
                "ET" => {
                    in_text = false;
                }
                "Tf" if in_text || true => {
                    // Set font — can appear outside BT/ET in some PDFs
                    if op.operands.len() >= 2 {
                        if let Ok(name_bytes) = op.operands[0].as_name() {
                            current_font_tag =
                                String::from_utf8_lossy(name_bytes).into_owned();
                        }
                        current_font_size = Self::obj_to_f64(&op.operands[1]).unwrap_or(12.0);
                    }
                }
                "Td" | "TD" => {
                    if op.operands.len() >= 2 {
                        let tx = Self::obj_to_f64(&op.operands[0]).unwrap_or(0.0);
                        let ty = Self::obj_to_f64(&op.operands[1]).unwrap_or(0.0);
                        x += tx;
                        y += ty;
                        tm_x = x;
                        tm_y = y;
                    }
                }
                "Tm" => {
                    // Text matrix: [a b c d e f]
                    if op.operands.len() >= 6 {
                        let e = Self::obj_to_f64(&op.operands[4]).unwrap_or(0.0);
                        let f = Self::obj_to_f64(&op.operands[5]).unwrap_or(0.0);
                        x = e;
                        y = f;
                        tm_x = e;
                        tm_y = f;
                    }
                }
                "T*" => {
                    // Move to start of next line (uses TL value; approximate)
                    y -= current_font_size * 1.2;
                    tm_y = y;
                }
                "Tj" => {
                    if let Some(text) = Self::extract_tj_text(&op.operands) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            let resolved = font_map
                                .get(current_font_tag.as_str())
                                .cloned()
                                .unwrap_or_else(|| current_font_tag.clone());
                            let font_name = if resolved.is_empty() {
                                "Unknown".to_string()
                            } else {
                                resolved
                            };
                            elements.push(RawElement::Text(RawTextBlock {
                                text: trimmed.to_string(),
                                x: tm_x,
                                y: tm_y,
                                font_size: current_font_size.abs(),
                                font_name,
                            }));
                        }
                    }
                }
                "TJ" => {
                    if let Some(text) = Self::extract_tj_array_text(&op.operands) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            let resolved = font_map
                                .get(current_font_tag.as_str())
                                .cloned()
                                .unwrap_or_else(|| current_font_tag.clone());
                            let font_name = if resolved.is_empty() {
                                "Unknown".to_string()
                            } else {
                                resolved
                            };
                            elements.push(RawElement::Text(RawTextBlock {
                                text: trimmed.to_string(),
                                x: tm_x,
                                y: tm_y,
                                font_size: current_font_size.abs(),
                                font_name,
                            }));
                        }
                    }
                }
                "'" | "\"" => {
                    // ' moves to next line then shows text; " sets word/char spacing then shows
                    y -= current_font_size * 1.2;
                    tm_y = y;
                    // The last operand is the string
                    if let Some(last) = op.operands.last() {
                        if let Ok(bytes) = last.as_str() {
                            let text = String::from_utf8_lossy(bytes);
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                let resolved = font_map
                                    .get(current_font_tag.as_str())
                                    .cloned()
                                    .unwrap_or_else(|| current_font_tag.clone());
                                let font_name = if resolved.is_empty() {
                                    "Unknown".to_string()
                                } else {
                                    resolved
                                };
                                elements.push(RawElement::Text(RawTextBlock {
                                    text: trimmed.to_string(),
                                    x: tm_x,
                                    y: tm_y,
                                    font_size: current_font_size.abs(),
                                    font_name,
                                }));
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(RawPage { elements })
    }

    /// Build a mapping from font resource names (e.g. "F1") to their /BaseFont names.
    fn build_font_map(doc: &Document, page_id: ObjectId) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();

        let fonts = match doc.get_page_fonts(page_id) {
            Ok(f) => f,
            Err(_) => return map,
        };

        for (name_bytes, font_dict) in &fonts {
            let tag = String::from_utf8_lossy(name_bytes).into_owned();
            let base_font = font_dict
                .get(b"BaseFont")
                .ok()
                .and_then(|obj| match obj {
                    Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
                    Object::Reference(id) => doc
                        .get_object(*id)
                        .ok()
                        .and_then(|o| o.as_name().ok())
                        .map(|n| String::from_utf8_lossy(n).into_owned()),
                    _ => None,
                })
                .unwrap_or_else(|| tag.clone());

            map.insert(tag, base_font);
        }

        map
    }

    /// Convert an lopdf Object (Integer or Real) to f64.
    fn obj_to_f64(obj: &Object) -> Option<f64> {
        match obj {
            Object::Real(f) => Some(*f as f64),
            Object::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Extract text from a Tj operand list.
    fn extract_tj_text(operands: &[Object]) -> Option<String> {
        for operand in operands {
            if let Object::String(bytes, _) = operand {
                return Some(String::from_utf8_lossy(bytes).into_owned());
            }
        }
        None
    }

    /// Extract text from a TJ array operand (array of strings and kerning numbers).
    fn extract_tj_array_text(operands: &[Object]) -> Option<String> {
        for operand in operands {
            if let Object::Array(items) = operand {
                let mut result = String::new();
                for item in items {
                    match item {
                        Object::String(bytes, _) => {
                            result.push_str(&String::from_utf8_lossy(bytes));
                        }
                        Object::Integer(i) if *i < -100 => {
                            result.push(' ');
                        }
                        _ => {}
                    }
                }
                return Some(result);
            }
        }
        None
    }

    /// Fallback: use doc.extract_text() when content stream parsing fails.
    fn extract_page_fallback(
        doc: &Document,
        page_num: u32,
    ) -> Result<RawPage, ConvertError> {
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

    #[test]
    fn test_obj_to_f64_real() {
        let obj = Object::Real(12.5);
        assert_eq!(PdfExtractor::obj_to_f64(&obj), Some(12.5));
    }

    #[test]
    fn test_obj_to_f64_integer() {
        let obj = Object::Integer(14);
        assert_eq!(PdfExtractor::obj_to_f64(&obj), Some(14.0));
    }

    #[test]
    fn test_obj_to_f64_invalid() {
        let obj = Object::Boolean(true);
        assert_eq!(PdfExtractor::obj_to_f64(&obj), None);
    }

    #[test]
    fn test_extract_tj_text() {
        let operands = vec![Object::String(b"Hello World".to_vec(), lopdf::StringFormat::Literal)];
        let text = PdfExtractor::extract_tj_text(&operands);
        assert_eq!(text, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_tj_text_empty() {
        let operands: Vec<Object> = vec![Object::Integer(42)];
        let text = PdfExtractor::extract_tj_text(&operands);
        assert_eq!(text, None);
    }

    #[test]
    fn test_extract_tj_array_text() {
        let operands = vec![Object::Array(vec![
            Object::String(b"Hel".to_vec(), lopdf::StringFormat::Literal),
            Object::Integer(-50),
            Object::String(b"lo".to_vec(), lopdf::StringFormat::Literal),
        ])];
        let text = PdfExtractor::extract_tj_array_text(&operands);
        assert_eq!(text, Some("Hello".to_string()));
    }

    #[test]
    fn test_extract_tj_array_with_large_kerning_inserts_space() {
        let operands = vec![Object::Array(vec![
            Object::String(b"Word1".to_vec(), lopdf::StringFormat::Literal),
            Object::Integer(-200),
            Object::String(b"Word2".to_vec(), lopdf::StringFormat::Literal),
        ])];
        let text = PdfExtractor::extract_tj_array_text(&operands);
        assert_eq!(text, Some("Word1 Word2".to_string()));
    }

    #[test]
    fn test_parse_pdf_date() {
        assert_eq!(
            PdfExtractor::parse_pdf_date("D:20240115120000"),
            "2024-01-15"
        );
    }

    #[test]
    fn test_parse_pdf_date_no_prefix() {
        assert_eq!(PdfExtractor::parse_pdf_date("20240115"), "2024-01-15");
    }

    #[test]
    fn test_parse_pdf_date_short() {
        assert_eq!(PdfExtractor::parse_pdf_date("D:2024"), "D:2024");
    }

    #[test]
    fn test_build_font_map_empty_doc() {
        // Just verifies it doesn't panic on an empty document
        let doc = Document::with_version("1.5");
        let map = PdfExtractor::build_font_map(&doc, (1, 0));
        assert!(map.is_empty());
    }

    #[test]
    fn test_metadata_nonexistent_file() {
        let meta = PdfExtractor::extract_metadata(Path::new("nonexistent.pdf"));
        assert!(meta.title.is_none());
        assert!(meta.author.is_none());
        assert!(meta.date.is_none());
    }
}
