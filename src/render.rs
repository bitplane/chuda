use std::{io::Write as _, path::Path};

use anyhow::{Context, Result};
use image::{GenericImageView, imageops::FilterType};

use crate::gpu;
#[cfg(test)]
use crate::symbols::{SYMBOLS, Symbol};

#[cfg(test)]
#[derive(Clone, Copy)]
struct Choice {
    ch: char,
    fg: [u8; 3],
    bg: [u8; 3],
}

pub fn render_png(
    path: &Path,
    columns: u32,
    font_ratio: f32,
    transparent_threshold: f32,
) -> Result<Vec<u8>> {
    let image = image::open(path).with_context(|| format!("decoding PNG {}", path.display()))?;
    let (w, h) = image.dimensions();
    let rows = ((h as f64 * columns as f64 / w as f64 / font_ratio as f64).round() as u32).max(1);
    let scaled = image
        .resize_exact(columns * 8, rows * 8, FilterType::Lanczos3)
        .to_rgba8();
    let mut input = Vec::with_capacity((columns * rows * 256) as usize);
    for cy in 0..rows {
        for cx in 0..columns {
            for y in 0..8 {
                for x in 0..8 {
                    input.extend_from_slice(&scaled.get_pixel(cx * 8 + x, cy * 8 + y).0);
                }
            }
        }
    }
    let choices = gpu::render(&input, transparent_threshold)?;
    let mut out = Vec::with_capacity((columns * rows * 32) as usize);
    for cy in 0..rows {
        let mut previous: Option<([u8; 3], Option<[u8; 3]>)> = None;
        for cx in 0..columns {
            let choice = choices[(cy * columns + cx) as usize];
            let bg = (choice.transparent_bg == 0).then_some(choice.bg);
            if previous != Some((choice.fg, bg)) {
                if let Some(bg) = bg {
                    write!(
                        out,
                        "\x1b[38;2;{};{};{};48;2;{};{};{}m",
                        choice.fg[0], choice.fg[1], choice.fg[2], bg[0], bg[1], bg[2]
                    )
                    .unwrap();
                } else {
                    write!(
                        out,
                        "\x1b[38;2;{};{};{};49m",
                        choice.fg[0], choice.fg[1], choice.fg[2]
                    )
                    .unwrap();
                }
                previous = Some((choice.fg, bg));
            }
            let mut utf8 = [0; 4];
            out.extend_from_slice(
                char::from_u32(choice.codepoint)
                    .unwrap_or(' ')
                    .encode_utf8(&mut utf8)
                    .as_bytes(),
            );
        }
        out.extend_from_slice(b"\x1b[0m\n");
    }
    Ok(out)
}

#[cfg(test)]
fn choose(pixels: &[[u8; 3]; 64]) -> Choice {
    let mut best = (
        u64::MAX,
        Choice {
            ch: ' ',
            fg: [0; 3],
            bg: [0; 3],
        },
    );
    for symbol in SYMBOLS {
        let candidate = evaluate(pixels, *symbol);
        if candidate.0 < best.0 {
            best = candidate;
        }
    }
    best.1
}

#[cfg(test)]
fn evaluate(pixels: &[[u8; 3]; 64], symbol: Symbol) -> (u64, Choice) {
    let mut sums = [[0u32; 3]; 2];
    let mut counts = [0u32; 2];
    for (i, pixel) in pixels.iter().enumerate() {
        let side = ((symbol.bitmap >> (63 - i)) & 1) as usize;
        counts[side] += 1;
        for c in 0..3 {
            sums[side][c] += pixel[c] as u32;
        }
    }
    let overall = [0, 1, 2].map(|c| (pixels.iter().map(|p| p[c] as u32).sum::<u32>() / 64) as u8);
    let colors = [0, 1].map(|side| {
        if counts[side] == 0 {
            overall
        } else {
            [0, 1, 2].map(|c| (sums[side][c] / counts[side]) as u8)
        }
    });
    let mut error = 0u64;
    for (i, pixel) in pixels.iter().enumerate() {
        let side = ((symbol.bitmap >> (63 - i)) & 1) as usize;
        for c in 0..3 {
            let d = pixel[c] as i32 - colors[side][c] as i32;
            error += (d * d) as u64;
        }
    }
    (
        error,
        Choice {
            ch: symbol.ch,
            fg: colors[1],
            bg: colors[0],
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_cell_uses_a_zero_error_symbol() {
        let pixels = [[12, 34, 56]; 64];
        let choice = choose(&pixels);
        assert_eq!(choice.ch, ' ');
        assert_eq!(choice.fg, [12, 34, 56]);
        assert_eq!(choice.bg, [12, 34, 56]);
    }
}
