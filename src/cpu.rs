use rayon::prelude::*;

use crate::{Choice, symbols::SYMBOLS};

pub fn render(pixels: &[u8], transparent_threshold: f32) -> Vec<Choice> {
    let edge_bias = (transparent_threshold * 64.0 * 65025.0 * 255.0 * 3.0) as u64;
    pixels
        .par_chunks_exact(256)
        .map(|cell| choose(cell, edge_bias))
        .collect()
}

fn choose(pixels: &[u8], edge_bias: u64) -> Choice {
    let mut best_error = u64::MAX;
    let mut best_index = usize::MAX;
    let mut best = Choice::default();
    for (index, symbol) in SYMBOLS.iter().enumerate() {
        let mut weight = [0u32; 2];
        let mut sum = [[0u32; 3]; 2];
        let mut sum_squares = [[0u32; 3]; 2];
        let mut alpha_error_opaque = 0u32;
        let mut alpha_error_edge = 0u32;
        for i in 0..64 {
            let side = ((symbol.bitmap >> (63 - i)) & 1) as usize;
            let alpha = pixels[i * 4 + 3] as u32;
            weight[side] += alpha;
            let opaque_delta = 255i32 - alpha as i32;
            let edge_delta = (if side == 1 { 255i32 } else { 0 }) - alpha as i32;
            alpha_error_opaque += (opaque_delta * opaque_delta) as u32;
            alpha_error_edge += (edge_delta * edge_delta) as u32;
            for channel in 0..3 {
                let value = pixels[i * 4 + channel] as u32;
                sum[side][channel] += alpha * value;
                sum_squares[side][channel] += alpha * value * value;
            }
        }
        let total_weight = weight[0] + weight[1];
        let mut opaque = Choice {
            codepoint: symbol.ch as u32,
            ..Choice::default()
        };
        let mut edge = opaque;
        edge.transparent_bg = 1;
        let mut opaque_error = 0u64;
        let mut edge_error = 0u64;
        for side in 0..2 {
            for channel in 0..3 {
                let mean = sum[side][channel]
                    .checked_div(weight[side])
                    .or_else(|| (sum[0][channel] + sum[1][channel]).checked_div(total_weight))
                    .unwrap_or(0);
                if side == 1 {
                    opaque.fg[channel] = mean as u8;
                    edge.fg[channel] = mean as u8;
                } else {
                    opaque.bg[channel] = mean as u8;
                }
                // CUDA evaluates this unsigned expression with wrapping
                // intermediates; the completed variance is non-negative.
                let rgb_error = (sum_squares[side][channel] as u64)
                    .wrapping_sub(2 * mean as u64 * sum[side][channel] as u64)
                    .wrapping_add(weight[side] as u64 * mean as u64 * mean as u64);
                opaque_error += rgb_error;
                if side == 1 {
                    edge_error += rgb_error;
                }
            }
        }
        opaque_error += 3 * 255 * alpha_error_opaque as u64;
        edge_error += 3 * 255 * alpha_error_edge as u64 + edge_bias;
        let (error, current) = if edge_error < opaque_error {
            (edge_error, edge)
        } else {
            (opaque_error, opaque)
        };
        if error < best_error || (error == best_error && index < best_index) {
            best_error = error;
            best_index = index;
            best = current;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_opaque_cell_uses_first_zero_error_symbol() {
        let mut pixels = [0u8; 256];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[12, 34, 56, 255]);
        }
        let choice = choose(&pixels, 0);
        assert_eq!(choice.codepoint, ' ' as u32);
        assert_eq!(choice.fg, [12, 34, 56]);
        assert_eq!(choice.bg, [12, 34, 56]);
        assert_eq!(choice.transparent_bg, 0);
    }
}
