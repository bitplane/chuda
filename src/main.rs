mod gpu;
mod render;
mod symbols;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// PNG file or a directory tree containing PNG files.
    input: PathBuf,

    /// Output cell width. Height is derived from image and terminal aspect ratios.
    #[arg(short = 's', long = "size", value_name = "WIDTH")]
    width: u32,

    /// Required for directory input. The input tree is mirrored here as .ansi files.
    #[arg(short, long, value_name = "DIR")]
    output: Option<PathBuf>,

    /// Cell pixel aspect ratio (terminal cells are normally twice as tall as wide).
    #[arg(long, default_value_t = 2.0)]
    font_ratio: f32,

    /// Bias toward detailed opaque cells. Edge transparency must improve the
    /// fit by roughly this fraction of an 8x8 cell to win.
    #[arg(long, default_value_t = 0.10)]
    transparent_threshold: f32,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.width == 0 {
        bail!("--size must be greater than zero");
    }
    if !(args.font_ratio.is_finite() && args.font_ratio > 0.0) {
        bail!("--font-ratio must be a positive finite number");
    }
    if !(0.0..=1.0).contains(&args.transparent_threshold) {
        bail!("--transparent-threshold must be between 0 and 1");
    }

    if args.input.is_file() {
        let ansi = render::render_png(
            &args.input,
            args.width,
            args.font_ratio,
            args.transparent_threshold,
        )?;
        use std::io::Write;
        std::io::stdout().lock().write_all(&ansi)?;
        return Ok(());
    }
    if !args.input.is_dir() {
        bail!("input does not exist: {}", args.input.display());
    }
    let output = args
        .output
        .as_ref()
        .context("--output is required for directory input")?;
    render_tree(
        &args.input,
        output,
        args.width,
        args.font_ratio,
        args.transparent_threshold,
    )
}

fn render_tree(
    root: &Path,
    output: &Path,
    width: u32,
    font_ratio: f32,
    transparent_threshold: f32,
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
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    for input in files {
        let relative = input.strip_prefix(root)?;
        let mut target = output.join(relative);
        target.set_extension("ansi");
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let ansi = render::render_png(&input, width, font_ratio, transparent_threshold)
            .with_context(|| format!("rendering {}", input.display()))?;
        fs::write(&target, ansi).with_context(|| format!("writing {}", target.display()))?;
    }
    Ok(())
}
