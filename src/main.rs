use any2md::converter::pdf::PdfConverter;
use any2md::converter::ConverterRegistry;
use any2md::model::options::{ConvertOptions, ImageMode, PageMode};
use any2md::renderer::markdown::MarkdownRenderer;
use clap::Parser;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Parser)]
#[command(name = "any2md", about = "Convert files to Markdown")]
struct Cli {
    /// Input file path
    input: PathBuf,

    /// Output file path (default: <input_name>.md)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Image mode: extract or inline
    #[arg(long, default_value = "extract")]
    images: String,

    /// Page mode: single or split
    #[arg(long, default_value = "single")]
    pages: String,
}

fn main() {
    let cli = Cli::parse();

    let image_mode = match cli.images.as_str() {
        "extract" => ImageMode::Extract,
        "inline" => ImageMode::Inline,
        other => {
            eprintln!(
                "Error: unknown image mode '{}'. Use 'extract' or 'inline'.",
                other
            );
            process::exit(1);
        }
    };

    let page_mode = match cli.pages.as_str() {
        "single" => PageMode::SingleFile,
        "split" => PageMode::SplitPages,
        other => {
            eprintln!(
                "Error: unknown page mode '{}'. Use 'single' or 'split'.",
                other
            );
            process::exit(1);
        }
    };

    let output_path = cli.output.unwrap_or_else(|| {
        let stem = cli.input.file_stem().unwrap_or_default();
        PathBuf::from(format!("{}.md", stem.to_string_lossy()))
    });

    let image_output_dir = output_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("images");

    let options = ConvertOptions {
        image_mode,
        page_mode,
        image_output_dir,
    };

    let mut registry = ConverterRegistry::new();
    registry.register(Box::new(PdfConverter));

    let ext = cli.input.extension().and_then(|e| e.to_str()).unwrap_or("");

    let converter = match registry.find_by_extension(ext) {
        Some(c) => c,
        None => {
            eprintln!("Error: unsupported format '.{}'", ext);
            process::exit(1);
        }
    };

    eprintln!("Converting {}...", cli.input.display());

    let doc = match converter.convert(&cli.input, &options) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };

    let markdown = match MarkdownRenderer::render(&doc, &options) {
        Ok(md) => md,
        Err(e) => {
            eprintln!("Error rendering: {}", e);
            process::exit(1);
        }
    };

    match std::fs::write(&output_path, &markdown) {
        Ok(_) => eprintln!("Written to {}", output_path.display()),
        Err(e) => {
            eprintln!("Error writing output: {}", e);
            process::exit(1);
        }
    }
}
