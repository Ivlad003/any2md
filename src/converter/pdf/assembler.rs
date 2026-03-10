use crate::converter::pdf::classifier::{BlockType, ClassifiedElement, Classifier};
use crate::converter::pdf::extractor::RawTextBlock;
use crate::model::document::*;

pub struct Assembler;

impl Assembler {
    pub fn assemble(
        classified_pages: Vec<Vec<ClassifiedElement>>,
        metadata: Metadata,
    ) -> Document {
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
                ClassifiedElement::Text(block, block_type) => {
                    match block_type {
                        BlockType::Heading(level) => {
                            elements.push(Element::Heading {
                                level: *level,
                                text: block.text.clone(),
                            });
                            i += 1;
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
                            elements.push(Element::Paragraph {
                                text: Self::rich_text_from_block(&block.text, block),
                            });
                            i += 1;
                        }
                    }
                }
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
                bold: Classifier::is_bold(&block.font_name),
                italic: Classifier::is_italic(&block.font_name),
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
        RawTextBlock {
            text: text.to_string(),
            x: 72.0,
            y: 700.0,
            font_size: 12.0,
            font_name: "Helvetica".to_string(),
        }
    }

    fn make_block_with_font(text: &str, font_name: &str) -> RawTextBlock {
        RawTextBlock {
            text: text.to_string(),
            x: 72.0,
            y: 700.0,
            font_size: 12.0,
            font_name: font_name.to_string(),
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
        assert!(matches!(&doc.pages[0].elements[0], Element::Paragraph { .. }));
        if let Element::Image { data, alt } = &doc.pages[0].elements[1] {
            assert_eq!(data, &vec![0x89, 0x50, 0x4E, 0x47]);
            assert!(alt.is_none());
        } else {
            panic!("Expected Image element");
        }
        assert!(matches!(&doc.pages[0].elements[2], Element::Paragraph { .. }));
    }
}
