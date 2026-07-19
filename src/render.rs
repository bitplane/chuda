use std::{io::Cursor, io::Write as _, path::Path, sync::Arc};

use anyhow::{Result, bail};
use image::{RgbaImage, imageops::FilterType};
use rayon::prelude::*;

use crate::{Backend, Choice};

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
    Ok(request.columns as usize * rows_for(request)? as usize)
}

fn rows_for(request: &RenderRequest<'_>) -> Result<u32> {
    let width = request.image.width();
    if width == 0 || request.image.height() == 0 {
        bail!("source image must not be empty");
    }
    Ok(((request.image.height() as f64 * request.columns as f64
        / width as f64
        / request.options.font_ratio as f64)
        .round() as u32)
        .max(1))
}

pub(crate) fn prepare_many(requests: &[RenderRequest<'_>]) -> Result<Vec<Prepared>> {
    requests.par_iter().map(prepare).collect()
}

fn prepare(request: &RenderRequest<'_>) -> Result<Prepared> {
    let rows = rows_for(request)?;
    let scaled = image::imageops::resize(
        request.image.rgba.as_ref(),
        request.columns * 8,
        rows * 8,
        FilterType::Lanczos3,
    );
    let mut pixels = Vec::with_capacity((request.columns * rows * 256) as usize);
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
    let mut out = Vec::with_capacity((columns * rows * 32) as usize);
    let mut previous_fg = None;
    let mut previous_bg = None;
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
mod tests {
    use super::*;

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
