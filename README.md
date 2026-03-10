# any2md

CLI utility in Rust for converting files to Markdown. Extensible trait-based plugin architecture where each input format implements a `Converter` trait.

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/any2md`.

## Usage

```
any2md <input> [options]

Options:
  -o, --output <path>            Output file (default: <input_name>.md)
      --images <extract|inline>  Image mode (default: extract)
      --pages <single|split>     Page mode (default: single)
      --debug                    Enable debug logging to console and file
      --log-file <path>          Path for debug log file (default: any2md.log)
  -h, --help                     Help
```

### Examples

```bash
# Convert PDF to Markdown
any2md document.pdf

# Custom output path
any2md document.pdf -o output/result.md

# Embed images as base64
any2md document.pdf --images inline

# Enable debug logging (outputs to stderr and any2md.log)
any2md document.pdf --debug

# Debug logging with custom log file
any2md document.pdf --debug --log-file /tmp/any2md-debug.log
```

## Supported Formats

| Format | Status | Notes |
|--------|--------|-------|
| PDF    | MVP    | Heuristic-based element classification |

## PDF Converter

The PDF converter uses a three-stage pipeline:

1. **Extraction** — Parses PDF content streams via `lopdf` to extract text blocks with font name, font size, and coordinates. Also extracts embedded images (XObjects).
2. **Classification** — Heuristics classify each text block:
   - **Code**: Monospace font (Courier, Consolas, Menlo, Monaco, etc.)
   - **Heading**: Font size significantly larger than baseline (H1/H2/H3 by ratio)
   - **List**: Line starts with bullet or ordered marker
   - **Paragraph**: Everything else (fallback)
   - **Bold/Italic**: Detected from font name (e.g., "Helvetica-Bold")
3. **Assembly** — Classified blocks are assembled into a `Document` model. Consecutive code blocks are merged. Consecutive list items are grouped.

### Image Handling

- `--images extract` (default): Saves images as PNG files to an `images/` directory and references them in markdown.
- `--images inline`: Embeds images as base64 data URIs.

### Metadata

PDF document metadata (title, author, creation date) is extracted from the document info dictionary and rendered as a header in the output.

## Architecture

```
Core
  trait Converter { fn convert(&self, input, options) -> Document }
  Document (unified intermediate model)
  MarkdownRenderer (Document -> .md)

Converters
  PdfConverter (Extractor -> Classifier -> Assembler)
```

All converters produce a unified `Document` model. One `MarkdownRenderer` generates Markdown from it. Adding a new format = new `impl Converter`, nothing else changes.

## Debug Logging

Pass `--debug` to enable detailed logging across the entire conversion pipeline. Logs are written simultaneously to stderr (with colors) and a log file (default: `any2md.log`).

| Component | Level | What is logged |
|-----------|-------|----------------|
| Extractor | `DEBUG` | Page count, font maps, content stream success/fallback, per-page text block and image counts |
| Extractor | `TRACE` | Individual PDF font-set operators (Tf) |
| Classifier | `DEBUG` | Baseline font size, page count |
| Classifier | `TRACE` | Every block classification decision: text preview, font name, font size, coordinates, result |
| PdfConverter | `INFO` | Pipeline start/complete with total element counts |
| PdfConverter | `DEBUG` | Each pipeline phase (extraction, classification, metadata, assembly) |
| Renderer | `DEBUG` | Rendering mode, image file saves with paths and byte sizes |
| CLI | `DEBUG`/`INFO` | Conversion options, converter selection, output path |

Without `--debug`, no logging overhead is added.

## Known Limitations

- **Table detection**: Not implemented. Tables in PDFs are rendered as paragraphs. The design calls for detection via aligned X-coordinates, which is planned for a future release.
- **Split pages mode**: `--pages split` is accepted but not yet implemented. Currently all pages render to a single file.
- **Password-protected PDFs**: Will fail with a generic parse error rather than a specific password-protection message.
- **Per-page progress**: Only a single "Converting..." message is shown. No per-page progress for large PDFs.
- **Nested lists**: Detected by list markers only, not by indentation analysis. All items are flat (no nesting from PDF source).
- **Font metadata**: Content stream parsing extracts font names and sizes, but some PDFs use embedded/subset fonts with non-standard naming that may not be recognized by the monospace or bold/italic heuristics.
- **Image formats**: Extracted images are saved as raw bytes with a `.png` extension. Some PDF images may use JPEG or other encodings that would need proper format detection.
- **Large PDFs**: All pages are loaded into memory. The design calls for page-by-page streaming for 1000+ page documents.

## Future Extensions

- HTML, DOCX, EPUB converters
- Audio-to-Markdown via speech-to-text
- Table detection via column alignment analysis
- Split pages mode
- Password-protected PDF support
- Streaming processing for large documents

## Development

```bash
# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```
