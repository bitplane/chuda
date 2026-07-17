use crate::symbols::SYMBOLS;
use anyhow::{Result, bail};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Choice {
    pub codepoint: u32,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub transparent_bg: u8,
}

unsafe extern "C" {
    fn cb_render_cuda(
        pixels: *const u8,
        cells: u32,
        masks: *const u64,
        codes: *const u32,
        symbols: u32,
        output: *mut Choice,
        transparent_threshold: f32,
        error: *mut u8,
        capacity: usize,
    ) -> i32;
}
pub fn render(pixels: &[u8], transparent_threshold: f32) -> Result<Vec<Choice>> {
    let cells = pixels.len() / 256;
    let masks: Vec<_> = SYMBOLS.iter().map(|s| s.bitmap).collect();
    let codes: Vec<_> = SYMBOLS.iter().map(|s| s.ch as u32).collect();
    let mut output = vec![Choice::default(); cells];
    let mut error = [0u8; 512];
    let status = unsafe {
        cb_render_cuda(
            pixels.as_ptr(),
            cells as u32,
            masks.as_ptr(),
            codes.as_ptr(),
            masks.len() as u32,
            output.as_mut_ptr(),
            transparent_threshold,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if status != 0 {
        let end = error.iter().position(|b| *b == 0).unwrap_or(error.len());
        bail!(
            "CUDA renderer failed: {}",
            String::from_utf8_lossy(&error[..end])
        );
    }
    Ok(output)
}
