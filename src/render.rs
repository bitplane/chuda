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
    Ok(encode_ansi(&choices, columns, rows))
}

fn encode_ansi(choices: &[gpu::Choice], columns: u32, rows: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((columns * rows * 32) as usize);
    let mut previous_fg: Option<[u8; 3]> = None;
    let mut previous_bg: Option<[u8; 3]> = None;
    for cy in 0..rows {
        for cx in 0..columns {
            let choice = choices[(cy * columns + cx) as usize];
            let bg = (choice.transparent_bg == 0).then_some(choice.bg);
            let fg_changed = previous_fg != Some(choice.fg);
            let bg_changed = previous_bg != bg;
            if fg_changed || bg_changed {
                out.extend_from_slice(b"\x1b[");
                if fg_changed {
                    write!(
                        out,
                        "38;2;{};{};{}",
                        choice.fg[0], choice.fg[1], choice.fg[2]
                    )
                    .unwrap();
                }
                if bg_changed {
                    if fg_changed {
                        out.push(b';');
                    }
                    if let Some(bg) = bg {
                        write!(out, "48;2;{};{};{}", bg[0], bg[1], bg[2]).unwrap();
                    } else {
                        out.extend_from_slice(b"49");
                    }
                }
                out.push(b'm');
                previous_fg = Some(choice.fg);
                previous_bg = bg;
            }
            let mut utf8 = [0; 4];
            out.extend_from_slice(
                char::from_u32(choice.codepoint)
                    .unwrap_or(' ')
                    .encode_utf8(&mut utf8)
                    .as_bytes(),
            );
        }
        if cy + 1 < rows {
            out.push(b'\n');
        }
    }
    out.extend_from_slice(b"\x1b[0m\n");
    out
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

    fn cell(ch: char, fg: [u8; 3], bg: Option<[u8; 3]>) -> gpu::Choice {
        gpu::Choice {
            codepoint: ch as u32,
            fg,
            bg: bg.unwrap_or_default(),
            transparent_bg: u8::from(bg.is_none()),
        }
    }

    #[test]
    fn ansi_emission_only_writes_changed_colour_fields() {
        let choices = [
            cell('a', [1, 2, 3], Some([4, 5, 6])),
            cell('b', [1, 2, 3], Some([7, 8, 9])),
            cell('c', [10, 11, 12], Some([7, 8, 9])),
            cell('d', [10, 11, 12], None),
        ];
        assert_eq!(
            encode_ansi(&choices, 4, 1),
            b"\x1b[38;2;1;2;3;48;2;4;5;6ma\x1b[48;2;7;8;9mb\x1b[38;2;10;11;12mc\x1b[49md\x1b[0m\n"
        );
    }

    #[test]
    fn ansi_emission_preserves_state_across_rows() {
        let choices = [
            cell('a', [1, 2, 3], Some([4, 5, 6])),
            cell('b', [1, 2, 3], Some([4, 5, 6])),
        ];
        assert_eq!(
            encode_ansi(&choices, 1, 2),
            b"\x1b[38;2;1;2;3;48;2;4;5;6ma\nb\x1b[0m\n"
        );
    }
}
