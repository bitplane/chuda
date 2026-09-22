use std::{io::Cursor, io::Write as _, path::Path, sync::Arc};

use anyhow::{Result, bail};
use image::{Rgba, Rgba32FImage, RgbaImage, imageops::FilterType};
#[cfg(feature = "cpu")]
use rayon::prelude::*;

use crate::{Backend, Choice};

// Bound a frame's cell-major RGBA buffer to 256 MiB before resizing.
const MAX_FRAME_CELLS: usize = 1_048_576;

#[derive(Clone)]
pub struct SourceImage {
    rgba: Arc<RgbaImage>,
}

impl SourceImage {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self::from_rgba(image::open(path)?.to_rgba8()))
    }

    pub fn from_png(bytes: &[u8]) -> Result<Self> {
        Ok(Self::from_rgba(
            image::load(Cursor::new(bytes), image::ImageFormat::Png)?.to_rgba8(),
        ))
    }

    pub fn from_raw(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let image = RgbaImage::from_raw(width, height, rgba)
            .ok_or_else(|| anyhow::anyhow!("RGBA buffer length does not match {width}x{height}"))?;
        Ok(Self::from_rgba(image))
    }

    pub fn from_rgba(rgba: RgbaImage) -> Self {
        Self {
            rgba: Arc::new(rgba),
        }
    }

    pub fn width(&self) -> u32 {
        self.rgba.width()
    }

    pub fn height(&self) -> u32 {
        self.rgba.height()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub font_ratio: f32,
    pub transparent_threshold: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            font_ratio: 2.0,
            transparent_threshold: 0.10,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RenderRequest<'a> {
    pub image: &'a SourceImage,
    pub columns: u32,
    pub options: RenderOptions,
}

impl<'a> RenderRequest<'a> {
    pub fn new(image: &'a SourceImage, columns: u32, options: RenderOptions) -> Self {
        Self {
            image,
            columns,
            options,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    columns: u32,
    rows: u32,
    backend: Backend,
    choices: Vec<Choice>,
}

impl Frame {
    pub fn columns(&self) -> u32 {
        self.columns
    }
    pub fn rows(&self) -> u32 {
        self.rows
    }
    pub fn backend(&self) -> Backend {
        self.backend
    }
    pub fn choices(&self) -> &[Choice] {
        &self.choices
    }
    pub fn to_ansi(&self) -> Vec<u8> {
        encode_ansi(&self.choices, self.columns, self.rows)
    }
}

pub(crate) struct Prepared {
    columns: u32,
    rows: u32,
    pub(crate) pixels: Vec<u8>,
}

pub(crate) fn validate_requests(requests: &[RenderRequest<'_>]) -> Result<()> {
    for request in requests {
        if request.columns == 0 {
            bail!("columns must be greater than zero");
        }
        if !(request.options.font_ratio.is_finite() && request.options.font_ratio > 0.0) {
            bail!("font_ratio must be a positive finite number");
        }
        if !(0.0..=1.0).contains(&request.options.transparent_threshold) {
            bail!("transparent_threshold must be between 0 and 1");
        }
        request_cells(request)?;
    }
    if let Some(first) = requests.first()
        && requests.iter().any(|request| {
            request.options.transparent_threshold != first.options.transparent_threshold
        })
    {
        bail!("all requests in a batch must use the same transparent_threshold");
    }
    Ok(())
}

pub(crate) fn request_cells(request: &RenderRequest<'_>) -> Result<usize> {
    let rows = rows_for(request)?;
    request
        .columns
        .checked_mul(8)
        .ok_or_else(|| anyhow::anyhow!("render width is too large"))?;
    rows.checked_mul(8)
        .ok_or_else(|| anyhow::anyhow!("render height is too large"))?;
    let cells = (request.columns as usize)
        .checked_mul(rows as usize)
        .filter(|&cells| cells <= MAX_FRAME_CELLS)
        .ok_or_else(|| anyhow::anyhow!("render exceeds the {MAX_FRAME_CELLS}-cell frame limit"))?;
    Ok(cells)
}

fn rows_for(request: &RenderRequest<'_>) -> Result<u32> {
    let width = request.image.width();
    if width == 0 || request.image.height() == 0 {
        bail!("source image must not be empty");
    }
    let rows = (request.image.height() as f64 * request.columns as f64
        / width as f64
        / request.options.font_ratio as f64)
        .round()
        .max(1.0);
    if !rows.is_finite() || rows > u32::MAX as f64 {
        bail!("render height is too large");
    }
    Ok(rows as u32)
}

pub(crate) fn prepare_many(requests: &[RenderRequest<'_>]) -> Result<Vec<Prepared>> {
    #[cfg(feature = "cpu")]
    {
        requests.par_iter().map(prepare).collect()
    }
    #[cfg(not(feature = "cpu"))]
    {
        requests.iter().map(prepare).collect()
    }
}

fn prepare(request: &RenderRequest<'_>) -> Result<Prepared> {
    let cells = request_cells(request)?;
    let rows = rows_for(request)?;
    let scaled = resize_rgba(request.image.rgba.as_ref(), request.columns * 8, rows * 8);
    let mut pixels = Vec::with_capacity(cells * 256);
    for cy in 0..rows {
        for cx in 0..request.columns {
            for y in 0..8 {
                for x in 0..8 {
                    pixels.extend_from_slice(&scaled.get_pixel(cx * 8 + x, cy * 8 + y).0);
                }
            }
        }
    }
    Ok(Prepared {
        columns: request.columns,
        rows,
        pixels,
    })
}

fn resize_rgba(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if source.dimensions() == (width, height) {
        return source.clone();
    }
    if source.pixels().all(|pixel| pixel[3] == 255) {
        return image::imageops::resize(source, width, height, FilterType::Lanczos3);
    }
    // Filter premultiplied floats so hidden RGB cannot bleed into sprite edges,
    // and low-alpha colors retain precision until the final conversion to u8.
    let premultiplied = Rgba32FImage::from_fn(source.width(), source.height(), |x, y| {
        let pixel = source.get_pixel(x, y);
        let alpha = pixel[3] as f32 / 255.0;
        Rgba([
            pixel[0] as f32 / 255.0 * alpha,
            pixel[1] as f32 / 255.0 * alpha,
            pixel[2] as f32 / 255.0 * alpha,
            alpha,
        ])
    });
    let scaled = image::imageops::resize(&premultiplied, width, height, FilterType::Lanczos3);
    RgbaImage::from_fn(width, height, |x, y| {
        let pixel = scaled.get_pixel(x, y);
        let alpha = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        if alpha == 0 {
            return Rgba([0; 4]);
        }
        Rgba([
            (pixel[0] / pixel[3] * 255.0).round().clamp(0.0, 255.0) as u8,
            (pixel[1] / pixel[3] * 255.0).round().clamp(0.0, 255.0) as u8,
            (pixel[2] / pixel[3] * 255.0).round().clamp(0.0, 255.0) as u8,
            alpha,
        ])
    })
}

pub(crate) fn split_frames(
    prepared: Vec<Prepared>,
    choices: Vec<Choice>,
    backend: Backend,
) -> Result<Vec<Frame>> {
    let expected: usize = prepared
        .iter()
        .map(|item| item.columns as usize * item.rows as usize)
        .sum();
    if choices.len() != expected {
        bail!(
            "backend returned {} cells, expected {expected}",
            choices.len()
        );
    }
    let mut offset = 0;
    Ok(prepared
        .into_iter()
        .map(|item| {
            let count = item.columns as usize * item.rows as usize;
            let frame = Frame {
                columns: item.columns,
                rows: item.rows,
                backend,
                choices: choices[offset..offset + count].to_vec(),
            };
            offset += count;
            frame
        })
        .collect())
}

fn encode_ansi(choices: &[Choice], columns: u32, rows: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(choices.len() * 32);
    let mut previous_fg = None;
    let mut previous_bg = None;
    for cy in 0..rows {
        for cx in 0..columns {
            let choice = choices[cy as usize * columns as usize + cx as usize];
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
mod tests {
    use super::*;

    #[test]
    fn oversized_requests_return_errors_before_allocation() {
        let image = SourceImage::from_raw(1, 1, vec![255; 4]).unwrap();
        let renderer = crate::Renderer::new(Backend::Cpu);
        for (columns, font_ratio) in [(536_870_912, 2.0), (1, 1e-30), (4096, 1.0)] {
            let options = RenderOptions {
                font_ratio,
                ..RenderOptions::default()
            };
            assert!(
                renderer
                    .render(RenderRequest::new(&image, columns, options))
                    .is_err()
            );
        }
        assert_eq!(
            request_cells(&RenderRequest::new(
                &image,
                1024,
                RenderOptions {
                    font_ratio: 1.0,
                    ..RenderOptions::default()
                }
            ))
            .unwrap(),
            MAX_FRAME_CELLS
        );
    }

    #[test]
    fn resizing_ignores_rgb_of_fully_transparent_pixels() {
        let source = |hidden| {
            RgbaImage::from_fn(64, 64, |x, y| {
                if (x + y) % 2 == 0 {
                    Rgba(hidden)
                } else {
                    Rgba([255, 0, 0, 255])
                }
            })
        };
        let red = resize_rgba(&source([255, 0, 0, 0]), 8, 8);
        let blue = resize_rgba(&source([0, 0, 255, 0]), 8, 8);
        assert_eq!(red, blue);
        for pixel in red.pixels() {
            assert_eq!(&pixel.0[..3], &[255, 0, 0]);
            assert!((126..=129).contains(&pixel[3]));
        }
    }

    fn cell(ch: char, fg: [u8; 3], bg: Option<[u8; 3]>) -> Choice {
        Choice {
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
}
