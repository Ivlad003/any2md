use any2md::model::document::*;
use any2md::model::options::*;
use any2md::renderer::markdown::MarkdownRenderer;

fn plain_text(s: &str) -> RichText {
    RichText {
        segments: vec![TextSegment {
            text: s.to_string(),
            bold: false,
            italic: false,
            code: false,
            link: None,
        }],
    }
}

#[test]
fn test_render_heading() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![
                Element::Heading { level: 1, text: "Title".to_string() },
                Element::Heading { level: 2, text: "Subtitle".to_string() },
                Element::Heading { level: 3, text: "Section".to_string() },
            ],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("# Title"));
    assert!(result.contains("## Subtitle"));
    assert!(result.contains("### Section"));
}

#[test]
fn test_render_paragraph_plain() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::Paragraph { text: plain_text("Hello world") }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("Hello world"));
}

#[test]
fn test_render_rich_text_formatting() {
    let rt = RichText {
        segments: vec![
            TextSegment { text: "bold".to_string(), bold: true, italic: false, code: false, link: None },
            TextSegment { text: " and ".to_string(), bold: false, italic: false, code: false, link: None },
            TextSegment { text: "italic".to_string(), bold: false, italic: true, code: false, link: None },
            TextSegment { text: " and ".to_string(), bold: false, italic: false, code: false, link: None },
            TextSegment { text: "code".to_string(), bold: false, italic: false, code: true, link: None },
            TextSegment { text: " and ".to_string(), bold: false, italic: false, code: false, link: None },
            TextSegment { text: "link".to_string(), bold: false, italic: false, code: false, link: Some("https://example.com".to_string()) },
        ],
    };
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::Paragraph { text: rt }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("**bold**"));
    assert!(result.contains("*italic*"));
    assert!(result.contains("`code`"));
    assert!(result.contains("[link](https://example.com)"));
}

#[test]
fn test_render_code_block() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::CodeBlock {
                language: Some("rust".to_string()),
                code: "fn main() {}".to_string(),
            }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("```rust\nfn main() {}\n```"));
}

#[test]
fn test_render_code_block_no_language() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::CodeBlock {
                language: None,
                code: "some code".to_string(),
            }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("```\nsome code\n```"));
}

#[test]
fn test_render_unordered_list() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::List {
                ordered: false,
                items: vec![
                    ListItem { text: plain_text("First"), children: vec![] },
                    ListItem { text: plain_text("Second"), children: vec![] },
                ],
            }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("- First"));
    assert!(result.contains("- Second"));
}

#[test]
fn test_render_ordered_list() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::List {
                ordered: true,
                items: vec![
                    ListItem { text: plain_text("First"), children: vec![] },
                    ListItem { text: plain_text("Second"), children: vec![] },
                ],
            }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("1. First"));
    assert!(result.contains("2. Second"));
}

#[test]
fn test_render_nested_list() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::List {
                ordered: false,
                items: vec![
                    ListItem {
                        text: plain_text("Parent"),
                        children: vec![
                            ListItem { text: plain_text("Child"), children: vec![] },
                        ],
                    },
                ],
            }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("- Parent"));
    assert!(result.contains("  - Child"));
}

#[test]
fn test_render_table() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::Table {
                headers: vec!["Name".to_string(), "Age".to_string()],
                rows: vec![
                    vec!["Alice".to_string(), "30".to_string()],
                    vec!["Bob".to_string(), "25".to_string()],
                ],
            }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("| Name | Age |"));
    assert!(result.contains("| --- | --- |"));
    assert!(result.contains("| Alice | 30 |"));
    assert!(result.contains("| Bob | 25 |"));
}

#[test]
fn test_render_horizontal_rule() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::HorizontalRule],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("---"));
}

#[test]
fn test_render_blockquote() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::BlockQuote { text: plain_text("A quote") }],
        }],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("> A quote"));
}

#[test]
fn test_render_image_inline() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![Page {
            elements: vec![Element::Image {
                data: vec![0x89, 0x50, 0x4E, 0x47],
                alt: Some("test image".to_string()),
            }],
        }],
    };
    let opts = ConvertOptions {
        image_mode: ImageMode::Inline,
        ..ConvertOptions::default()
    };
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("![test image](data:image/png;base64,"));
}

#[test]
fn test_render_multiple_pages_single_file() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![
            Page { elements: vec![Element::Paragraph { text: plain_text("Page 1") }] },
            Page { elements: vec![Element::Paragraph { text: plain_text("Page 2") }] },
        ],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("Page 1"));
    assert!(result.contains("<!-- page 2 -->"));
    assert!(result.contains("Page 2"));
}

#[test]
fn test_render_metadata_header() {
    let doc = Document {
        metadata: Metadata {
            title: Some("My Doc".to_string()),
            author: Some("Author".to_string()),
            date: Some("2026-01-01".to_string()),
        },
        pages: vec![],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("# My Doc"));
    assert!(result.contains("Author"));
    assert!(result.contains("2026-01-01"));
}

#[test]
fn test_render_empty_page_skipped() {
    let doc = Document {
        metadata: Metadata { title: None, author: None, date: None },
        pages: vec![
            Page { elements: vec![] },
            Page { elements: vec![Element::Paragraph { text: plain_text("Content") }] },
        ],
    };
    let opts = ConvertOptions::default();
    let result = MarkdownRenderer::render(&doc, &opts);
    assert!(result.contains("Content"));
    assert!(!result.contains("<!-- page 1 -->"));
}
