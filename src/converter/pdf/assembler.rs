use crate::converter::pdf::classifier::{BlockType, ClassifiedElement};
use crate::converter::pdf::extractor::RawTextBlock;
use crate::model::document::*;

pub struct Assembler;

impl Assembler {
    pub fn assemble(classified_pages: Vec<Vec<ClassifiedElement>>, metadata: Metadata) -> Document {
        let pages = classified_pages
            .into_iter()
            .map(Self::assemble_page)
            .collect();

        Document { metadata, pages }
    }

    fn assemble_page(elems: Vec<ClassifiedElement>) -> Page {
        let mut elements = Vec::new();
        let mut i = 0;

        while i < elems.len() {
            match &elems[i] {
                ClassifiedElement::Image(img) => {
                    elements.push(Element::Image {
                        data: img.data.clone(),
                        alt: None,
                    });
                    i += 1;
                }
                ClassifiedElement::PreBuilt(el) => {
                    elements.push(el.clone());
                    i += 1;
                }
                ClassifiedElement::Text(block, block_type) => match block_type {
                    BlockType::Heading(level) => {
                        let mut heading_text = block.text.clone();
                        let heading_level = *level;
                        let mut last_y = block.y;
                        let last_font_size = block.font_size;
                        i += 1;
                        // Merge consecutive headings at the same level (wrapped text)
                        while i < elems.len() {
                            if let ClassifiedElement::Text(
                                next_block,
                                BlockType::Heading(next_level),
                            ) = &elems[i]
                            {
                                if *next_level == heading_level {
                                    let y_gap = (next_block.y - last_y).abs();
                                    if y_gap < last_font_size * 2.0 {
                                        heading_text.push(' ');
                                        heading_text.push_str(&next_block.text);
                                        last_y = next_block.y;
                                        i += 1;
                                        continue;
                                    }
                                }
                            }
                            break;
                        }
                        elements.push(Element::Heading {
                            level: heading_level,
                            text: heading_text,
                        });
                    }
                    BlockType::CodeBlock => {
                        let mut code_lines = vec![block.text.clone()];
                        i += 1;
                        while i < elems.len() {
                            if let ClassifiedElement::Text(b, BlockType::CodeBlock) = &elems[i] {
                                code_lines.push(b.text.clone());
                                i += 1;
                            } else {
                                break;
                            }
                        }
                        elements.push(Element::CodeBlock {
                            language: None,
                            code: code_lines.join("\n"),
                        });
                    }
                    BlockType::ListItem => {
                        let start = i;
                        let mut items = Vec::new();
                        while i < elems.len() {
                            if let ClassifiedElement::Text(b, BlockType::ListItem) = &elems[i] {
                                let text = Self::strip_list_marker(&b.text);
                                items.push(ListItem {
                                    text: Self::rich_text_from_block(&text, b),
                                    children: vec![],
                                });
                                i += 1;
                            } else {
                                break;
                            }
                        }
                        let first_text = if let ClassifiedElement::Text(b, _) = &elems[start] {
                            b.text.as_str()
                        } else {
                            ""
                        };
                        let ordered = Self::detect_ordered(first_text);
                        elements.push(Element::List { ordered, items });
                    }
                    BlockType::Paragraph => {
                        let mut para_text = block.text.clone();
                        let mut current_y = block.y;
                        i += 1;

                        // Merge URL continuations: if text contains "://" and ends with '-',
                        // the next paragraph at same X is likely a wrapped URL
                        while i < elems.len() {
                            if let ClassifiedElement::Text(next_block, BlockType::Paragraph) =
                                &elems[i]
                            {
                                let y_gap = (next_block.y - current_y).abs();
                                let same_x = (next_block.x - block.x).abs() < 5.0;
                                let line_height = block.font_size * 1.5;

                                if same_x
                                    && y_gap < line_height
                                    && para_text.contains("://")
                                    && para_text.ends_with('-')
                                {
                                    para_text.push_str(&next_block.text);
                                    current_y = next_block.y;
                                    i += 1;
                                    continue;
                                }
                            }
                            break;
                        }

                        elements.push(Element::Paragraph {
                            text: Self::rich_text_from_block(&para_text, block),
                        });
                    }
                },
            }
        }

        Page { elements }
    }

    fn strip_list_marker(text: &str) -> String {
        let text = text.trim();
        if let Some(rest) = text.strip_prefix("• ") {
            return rest.to_string();
        }
        if text.starts_with("- ") || text.starts_with("* ") {
            return text[2..].to_string();
        }
        if let Some(pos) = text.find(". ") {
            let prefix = &text[..pos];
            if prefix.chars().all(|c| c.is_ascii_digit()) {
                return text[pos + 2..].to_string();
            }
        }
        if let Some(pos) = text.find(") ") {
            let prefix = &text[..pos];
            if prefix.chars().all(|c| c.is_ascii_digit()) {
                return text[pos + 2..].to_string();
            }
        }
        text.to_string()
    }

    fn detect_ordered(text: &str) -> bool {
        let trimmed = text.trim();
        trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    }

    fn rich_text_from_block(text: &str, block: &RawTextBlock) -> RichText {
        RichText {
            segments: vec![TextSegment {
                text: text.to_string(),
                bold: block.has_bold,
                italic: block.has_italic,
                code: false,
                link: None,
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::pdf::extractor::RawTextBlock;

    fn make_block(text: &str) -> RawTextBlock {
        let end_x = 72.0 + text.chars().count() as f64 * 12.0 * 0.5;
        RawTextBlock {
            text: text.to_string(),
            x: 72.0,
            y: 700.0,
            end_x,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        }
    }

    fn make_block_with_font(text: &str, font_name: &str) -> RawTextBlock {
        let end_x = 72.0 + text.chars().count() as f64 * 12.0 * 0.5;
        let font_lower = font_name.to_lowercase();
        RawTextBlock {
            text: text.to_string(),
            x: 72.0,
            y: 700.0,
            end_x,
            font_size: 12.0,
            font_name: font_name.to_string(),
            has_bold: font_lower.contains("bold"),
            has_italic: font_lower.contains("italic") || font_lower.contains("oblique"),
        }
    }

    fn empty_metadata() -> Metadata {
        Metadata {
            title: None,
            author: None,
            date: None,
        }
    }

    fn ce(block: RawTextBlock, bt: BlockType) -> ClassifiedElement {
        ClassifiedElement::Text(block, bt)
    }

    #[test]
    fn test_assemble_headings() {
        let blocks = vec![
            ce(make_block("Title"), BlockType::Heading(1)),
            ce(make_block("Subtitle"), BlockType::Heading(2)),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages.len(), 1);
        assert!(
            matches!(&doc.pages[0].elements[0], Element::Heading { level: 1, text } if text == "Title")
        );
        assert!(
            matches!(&doc.pages[0].elements[1], Element::Heading { level: 2, text } if text == "Subtitle")
        );
    }

    #[test]
    fn test_assemble_consecutive_code_blocks_merged() {
        let blocks = vec![
            ce(make_block("fn main() {"), BlockType::CodeBlock),
            ce(make_block("    println!(\"hi\");"), BlockType::CodeBlock),
            ce(make_block("}"), BlockType::CodeBlock),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages[0].elements.len(), 1);
        if let Element::CodeBlock { code, .. } = &doc.pages[0].elements[0] {
            assert!(code.contains("fn main()"));
            assert!(code.contains("println!"));
            assert!(code.contains("}"));
        } else {
            panic!("Expected CodeBlock");
        }
    }

    #[test]
    fn test_assemble_list_items() {
        let blocks = vec![
            ce(make_block("- First"), BlockType::ListItem),
            ce(make_block("- Second"), BlockType::ListItem),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        if let Element::List { ordered, items } = &doc.pages[0].elements[0] {
            assert!(!ordered);
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].text.segments[0].text, "First");
        } else {
            panic!("Expected List");
        }
    }

    #[test]
    fn test_assemble_ordered_list() {
        let blocks = vec![
            ce(make_block("1. First"), BlockType::ListItem),
            ce(make_block("2. Second"), BlockType::ListItem),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        if let Element::List { ordered, items } = &doc.pages[0].elements[0] {
            assert!(ordered);
            assert_eq!(items.len(), 2);
        } else {
            panic!("Expected ordered List");
        }
    }

    #[test]
    fn test_assemble_paragraphs() {
        let blocks = vec![
            ce(make_block("Some text"), BlockType::Paragraph),
            ce(make_block("More text"), BlockType::Paragraph),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages[0].elements.len(), 2);
        assert!(matches!(
            &doc.pages[0].elements[0],
            Element::Paragraph { .. }
        ));
    }

    #[test]
    fn test_assemble_mixed_content() {
        let blocks = vec![
            ce(make_block("Title"), BlockType::Heading(1)),
            ce(make_block("Intro text"), BlockType::Paragraph),
            ce(make_block("let x = 1;"), BlockType::CodeBlock),
            ce(make_block("- item"), BlockType::ListItem),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages[0].elements.len(), 4);
    }

    #[test]
    fn test_strip_list_markers() {
        assert_eq!(Assembler::strip_list_marker("- item"), "item");
        assert_eq!(Assembler::strip_list_marker("• item"), "item");
        assert_eq!(Assembler::strip_list_marker("* item"), "item");
        assert_eq!(Assembler::strip_list_marker("1. item"), "item");
        assert_eq!(Assembler::strip_list_marker("10. item"), "item");
    }

    #[test]
    fn test_bold_font_produces_bold_paragraph() {
        let blocks = vec![ce(
            make_block_with_font("Bold text", "Helvetica-Bold"),
            BlockType::Paragraph,
        )];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        if let Element::Paragraph { text } = &doc.pages[0].elements[0] {
            assert!(text.segments[0].bold);
            assert!(!text.segments[0].italic);
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_italic_font_produces_italic_paragraph() {
        let blocks = vec![ce(
            make_block_with_font("Italic text", "Helvetica-Oblique"),
            BlockType::Paragraph,
        )];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        if let Element::Paragraph { text } = &doc.pages[0].elements[0] {
            assert!(!text.segments[0].bold);
            assert!(text.segments[0].italic);
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_bold_italic_font_produces_both() {
        let blocks = vec![ce(
            make_block_with_font("Bold italic text", "Helvetica-BoldOblique"),
            BlockType::Paragraph,
        )];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        if let Element::Paragraph { text } = &doc.pages[0].elements[0] {
            assert!(text.segments[0].bold);
            assert!(text.segments[0].italic);
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_plain_font_no_bold_no_italic() {
        let blocks = vec![ce(
            make_block_with_font("Plain text", "Helvetica"),
            BlockType::Paragraph,
        )];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        if let Element::Paragraph { text } = &doc.pages[0].elements[0] {
            assert!(!text.segments[0].bold);
            assert!(!text.segments[0].italic);
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_consecutive_headings_same_level_merged() {
        let b1 = RawTextBlock {
            text: "Client - Predefined product list (To review by".to_string(),
            x: 50.0,
            y: 700.0,
            end_x: 600.0,
            font_size: 26.7,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        };
        let b2 = RawTextBlock {
            text: "PICTO)".to_string(),
            x: 50.0,
            y: 720.0,
            end_x: 130.0,
            font_size: 26.7,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        };
        let blocks = vec![
            ClassifiedElement::Text(b1, BlockType::Heading(2)),
            ClassifiedElement::Text(b2, BlockType::Heading(2)),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages[0].elements.len(), 1);
        if let Element::Heading { level, text } = &doc.pages[0].elements[0] {
            assert_eq!(*level, 2);
            assert!(text.contains("Client"));
            assert!(text.contains("PICTO)"));
        } else {
            panic!("Expected merged heading");
        }
    }

    #[test]
    fn test_assemble_image_element() {
        use crate::converter::pdf::extractor::RawImage;
        let blocks = vec![
            ce(make_block("Before image"), BlockType::Paragraph),
            ClassifiedElement::Image(RawImage {
                data: vec![0x89, 0x50, 0x4E, 0x47],
                width: 200,
                height: 100,
            }),
            ce(make_block("After image"), BlockType::Paragraph),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages[0].elements.len(), 3);
        assert!(matches!(
            &doc.pages[0].elements[0],
            Element::Paragraph { .. }
        ));
        if let Element::Image { data, alt } = &doc.pages[0].elements[1] {
            assert_eq!(data, &vec![0x89, 0x50, 0x4E, 0x47]);
            assert!(alt.is_none());
        } else {
            panic!("Expected Image element");
        }
        assert!(matches!(
            &doc.pages[0].elements[2],
            Element::Paragraph { .. }
        ));
    }

    #[test]
    fn test_url_continuation_merged_across_line_break() {
        let b1 = RawTextBlock {
            text: "https://example.com/very/long/path/that-".to_string(),
            x: 72.0,
            y: 700.0,
            end_x: 500.0,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        };
        let b2 = RawTextBlock {
            text: "continues/here".to_string(),
            x: 72.0,
            y: 714.0, // within 1.5 * font_size = 18.0
            end_x: 200.0,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        };
        let blocks = vec![
            ClassifiedElement::Text(b1, BlockType::Paragraph),
            ClassifiedElement::Text(b2, BlockType::Paragraph),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages[0].elements.len(), 1);
        if let Element::Paragraph { text } = &doc.pages[0].elements[0] {
            assert_eq!(
                text.segments[0].text,
                "https://example.com/very/long/path/that-continues/here"
            );
        } else {
            panic!("Expected merged Paragraph");
        }
    }

    #[test]
    fn test_url_continuation_not_merged_without_protocol() {
        // Without "://" the paragraphs should NOT be merged even if text ends with '-'
        let b1 = RawTextBlock {
            text: "some regular text that ends with a hyphen-".to_string(),
            x: 72.0,
            y: 700.0,
            end_x: 500.0,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        };
        let b2 = RawTextBlock {
            text: "ated word".to_string(),
            x: 72.0,
            y: 714.0,
            end_x: 200.0,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
            has_bold: false,
            has_italic: false,
        };
        let blocks = vec![
            ClassifiedElement::Text(b1, BlockType::Paragraph),
            ClassifiedElement::Text(b2, BlockType::Paragraph),
        ];
        let doc = Assembler::assemble(vec![blocks], empty_metadata());
        assert_eq!(doc.pages[0].elements.len(), 2);
    }
}
