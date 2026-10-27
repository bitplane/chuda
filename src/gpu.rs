use crate::{Choice, symbols::SYMBOLS};
use anyhow::{Result, bail};
use std::sync::Mutex;

static CUDA_LOCK: Mutex<()> = Mutex::new(());

unsafe extern "C" {
    fn cb_cuda_available(error: *mut u8, capacity: usize) -> i32;
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

pub fn available() -> Result<()> {
    let mut error = [0u8; 512];
    let status = unsafe { cb_cuda_available(error.as_mut_ptr(), error.len()) };
    if status != 0 {
        let end = error.iter().position(|b| *b == 0).unwrap_or(error.len());
        bail!("{}", String::from_utf8_lossy(&error[..end]));
    }
    Ok(())
}
pub fn render(pixels: &[u8], transparent_threshold: f32) -> Result<Vec<Choice>> {
    let _guard = CUDA_LOCK.lock().expect("CUDA renderer lock poisoned");
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

#[cfg(all(test, feature = "cpu"))]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn cudaGetDeviceCount(count: *mut i32) -> i32;
        fn cudaGetDevice(device: *mut i32) -> i32;
        fn cudaSetDevice(device: i32) -> i32;
    }

    #[test]
    #[ignore = "requires two working NVIDIA GPUs"]
    fn cached_buffers_follow_the_current_device() {
        let mut count = 0;
        let mut original = 0;
        unsafe {
            assert_eq!(cudaGetDeviceCount(&mut count), 0);
            assert!(count >= 2, "requires two working NVIDIA GPUs");
            assert_eq!(cudaGetDevice(&mut original), 0);
        }
        struct RestoreDevice(i32);
        impl Drop for RestoreDevice {
            fn drop(&mut self) {
                unsafe {
                    cudaSetDevice(self.0);
                }
            }
        }
        let _restore = RestoreDevice(original);
        // Revisit each cache with different sizes and nonuniform pixels.
        for (device, cells) in [(0, 2), (1, 1), (0, 1), (1, 3)] {
            unsafe {
                assert_eq!(cudaSetDevice(device), 0);
            }
            let pixels: Vec<u8> = (0..cells * 256)
                .map(|i| ((i * 37 + device as usize * 13) % 256) as u8)
                .collect();
            assert_eq!(
                render(&pixels, 0.1).unwrap(),
                crate::cpu::render(&pixels, 0.1)
            );
        }
    }
}
