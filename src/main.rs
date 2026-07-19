use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chuda::{Backend, RenderOptions, RenderRequest, Renderer, SourceImage};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    input: PathBuf,
    #[arg(short = 's', long = "size", value_name = "WIDTH")]
    width: u32,
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = 2.0)]
    font_ratio: f32,
    #[arg(long, default_value_t = 0.10)]
    transparent_threshold: f32,
    #[arg(long, default_value = "auto")]
    backend: Backend,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let options = RenderOptions {
        font_ratio: args.font_ratio,
        transparent_threshold: args.transparent_threshold,
    };
    let renderer = Renderer::new(args.backend);
    if args.input.is_file() {
        let image = SourceImage::open(&args.input)?;
        let frame = renderer.render(RenderRequest::new(&image, args.width, options))?;
        warn_fallback(&renderer);
        std::io::stdout().lock().write_all(&frame.to_ansi())?;
        return Ok(());
    }
    if !args.input.is_dir() {
        bail!("input does not exist: {}", args.input.display());
    }
    let output = args
        .output
        .as_ref()
        .context("--output is required for directory input")?;
    render_tree(&renderer, &args.input, output, args.width, options)
}

fn warn_fallback(renderer: &Renderer) {
    if let Some(warning) = renderer.take_fallback_warning() {
        eprintln!("warning: {warning}");
    }
}

fn render_tree(
    renderer: &Renderer,
    root: &Path,
    output: &Path,
    width: u32,
    options: RenderOptions,
) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    for input in files {
        let image =
            SourceImage::open(&input).with_context(|| format!("rendering {}", input.display()))?;
        let frame = renderer.render(RenderRequest::new(&image, width, options))?;
        warn_fallback(renderer);
        let mut target = output.join(input.strip_prefix(root)?);
        target.set_extension("ansi");
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, frame.to_ansi())
            .with_context(|| format!("writing {}", target.display()))?;
    }
    Ok(())
}
