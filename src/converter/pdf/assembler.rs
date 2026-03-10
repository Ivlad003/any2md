use crate::converter::pdf::classifier::BlockType;
use crate::converter::pdf::extractor::RawTextBlock;
use crate::model::document::*;

pub struct Assembler;

impl Assembler {
    pub fn assemble(
        classified_pages: Vec<Vec<(RawTextBlock, BlockType)>>,
        metadata: Metadata,
    ) -> Document {
        let pages = classified_pages
            .into_iter()
            .map(Self::assemble_page)
            .collect();

        Document { metadata, pages }
    }

    fn assemble_page(blocks: Vec<(RawTextBlock, BlockType)>) -> Page {
        let mut elements = Vec::new();
        let mut i = 0;

        while i < blocks.len() {
            let (ref block, ref block_type) = blocks[i];

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
                    while i < blocks.len() {
                        if let BlockType::CodeBlock = blocks[i].1 {
                            code_lines.push(blocks[i].0.text.clone());
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
                    let mut items = Vec::new();
                    while i < blocks.len() {
                        if let BlockType::ListItem = blocks[i].1 {
                            let text = Self::strip_list_marker(&blocks[i].0.text);
                            items.push(ListItem {
                                text: Self::plain_rich_text(&text),
                                children: vec![],
                            });
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    let ordered = Self::detect_ordered(
                        blocks
                            .get(i.saturating_sub(items.len()))
                            .map(|(b, _)| b.text.as_str())
                            .unwrap_or(""),
                    );
                    elements.push(Element::List { ordered, items });
                }
                BlockType::Paragraph => {
                    elements.push(Element::Paragraph {
                        text: Self::plain_rich_text(&block.text),
                    });
                    i += 1;
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

    fn plain_rich_text(text: &str) -> RichText {
        RichText {
            segments: vec![TextSegment {
                text: text.to_string(),
                bold: false,
                italic: false,
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

    fn empty_metadata() -> Metadata {
        Metadata {
            title: None,
            author: None,
            date: None,
        }
    }

    #[test]
    fn test_assemble_headings() {
        let blocks = vec![
            (make_block("Title"), BlockType::Heading(1)),
            (make_block("Subtitle"), BlockType::Heading(2)),
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
            (make_block("fn main() {"), BlockType::CodeBlock),
            (make_block("    println!(\"hi\");"), BlockType::CodeBlock),
            (make_block("}"), BlockType::CodeBlock),
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
            (make_block("- First"), BlockType::ListItem),
            (make_block("- Second"), BlockType::ListItem),
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
            (make_block("1. First"), BlockType::ListItem),
            (make_block("2. Second"), BlockType::ListItem),
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
            (make_block("Some text"), BlockType::Paragraph),
            (make_block("More text"), BlockType::Paragraph),
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
            (make_block("Title"), BlockType::Heading(1)),
            (make_block("Intro text"), BlockType::Paragraph),
            (make_block("let x = 1;"), BlockType::CodeBlock),
            (make_block("- item"), BlockType::ListItem),
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
}
