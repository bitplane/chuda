mod render;
mod symbols;

#[cfg(feature = "cpu")]
mod cpu;
#[cfg(feature = "cuda")]
mod gpu;

use std::{fmt, path::Path, sync::Mutex};

use anyhow::{Context, Result, anyhow, bail};
pub use render::{Frame, RenderOptions, RenderRequest, SourceImage};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub codepoint: u32,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub transparent_bg: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    #[default]
    Auto,
    Cpu,
    Cuda,
}

impl fmt::Display for Backend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Auto => "auto",
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
        })
    }
}

impl std::str::FromStr for Backend {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" => Ok(Self::Cpu),
            "cuda" => Ok(Self::Cuda),
            _ => bail!("backend must be auto, cpu, or cuda"),
        }
    }
}

#[derive(Debug)]
struct RendererState {
    cuda_disabled: Option<String>,
    fallback: Option<String>,
}

pub struct Renderer {
    requested: Backend,
    max_batch_cells: usize,
    state: Mutex<RendererState>,
}

impl Renderer {
    pub fn new(backend: Backend) -> Self {
        Self::with_batch_limit(backend, 262_144)
    }

    pub fn with_batch_limit(backend: Backend, max_batch_cells: usize) -> Self {
        Self {
            requested: backend,
            max_batch_cells: max_batch_cells.max(1),
            state: Mutex::new(RendererState {
                cuda_disabled: None,
                fallback: None,
            }),
        }
    }

    pub fn backend(&self) -> Backend {
        self.requested
    }

    pub fn take_fallback_warning(&self) -> Option<String> {
        self.state
            .lock()
            .expect("renderer state poisoned")
            .fallback
            .take()
    }

    pub fn render(&self, request: RenderRequest<'_>) -> Result<Frame> {
        self.render_many(&[request])?
            .pop()
            .ok_or_else(|| anyhow!("renderer returned no frame"))
    }

    pub fn render_many(&self, requests: &[RenderRequest<'_>]) -> Result<Vec<Frame>> {
        render::validate_requests(requests)?;
        let mut frames = Vec::with_capacity(requests.len());
        let mut start = 0;
        while start < requests.len() {
            let mut end = start;
            let mut cells = 0usize;
            while end < requests.len() {
                let request_cells = render::request_cells(&requests[end])?;
                if end > start && cells + request_cells > self.max_batch_cells {
                    break;
                }
                cells += request_cells;
                end += 1;
            }
            frames.extend(self.render_chunk(&requests[start..end])?);
            start = end;
        }
        Ok(frames)
    }

    fn render_chunk(&self, requests: &[RenderRequest<'_>]) -> Result<Vec<Frame>> {
        let prepared = render::prepare_many(requests)?;
        let mut pixels = Vec::with_capacity(prepared.iter().map(|item| item.pixels.len()).sum());
        for item in &prepared {
            pixels.extend_from_slice(&item.pixels);
        }

        let (choices, actual) = match self.requested {
            Backend::Cpu => (
                render_cpu(&pixels, requests[0].options.transparent_threshold)?,
                Backend::Cpu,
            ),
            Backend::Cuda => (
                render_cuda(&pixels, requests[0].options.transparent_threshold)?,
                Backend::Cuda,
            ),
            Backend::Auto => {
                self.render_auto(&pixels, requests[0].options.transparent_threshold)?
            }
        };
        render::split_frames(prepared, choices, actual)
    }

    fn render_auto(&self, pixels: &[u8], threshold: f32) -> Result<(Vec<Choice>, Backend)> {
        let disabled = self
            .state
            .lock()
            .expect("renderer state poisoned")
            .cuda_disabled
            .is_some();
        if !disabled {
            match render_cuda(pixels, threshold) {
                Ok(choices) => return Ok((choices, Backend::Cuda)),
                Err(error) => {
                    let reason = error.to_string();
                    let mut state = self.state.lock().expect("renderer state poisoned");
                    state.cuda_disabled = Some(reason.clone());
                    state.fallback =
                        Some(format!("CUDA unavailable ({reason}); falling back to CPU"));
                }
            }
        }
        Ok((render_cpu(pixels, threshold)?, Backend::Cpu))
    }
}

#[cfg(feature = "cpu")]
fn render_cpu(pixels: &[u8], threshold: f32) -> Result<Vec<Choice>> {
    Ok(cpu::render(pixels, threshold))
}

#[cfg(not(feature = "cpu"))]
fn render_cpu(_pixels: &[u8], _threshold: f32) -> Result<Vec<Choice>> {
    bail!("Chuda was built without the CPU backend")
}

#[cfg(feature = "cuda")]
fn render_cuda(pixels: &[u8], threshold: f32) -> Result<Vec<Choice>> {
    gpu::available().context("CUDA initialization failed")?;
    gpu::render(pixels, threshold)
}

#[cfg(not(feature = "cuda"))]
fn render_cuda(_pixels: &[u8], _threshold: f32) -> Result<Vec<Choice>> {
    bail!("Chuda was built without the CUDA backend")
}

pub fn render_png(
    path: &Path,
    columns: u32,
    options: RenderOptions,
    backend: Backend,
) -> Result<Frame> {
    let image =
        SourceImage::open(path).with_context(|| format!("decoding PNG {}", path.display()))?;
    Renderer::new(backend).render(RenderRequest::new(&image, columns, options))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "cuda")]
    use std::path::PathBuf;

    #[test]
    fn backend_names_parse() {
        assert_eq!("auto".parse::<Backend>().unwrap(), Backend::Auto);
        assert_eq!("CPU".parse::<Backend>().unwrap(), Backend::Cpu);
        assert!("other".parse::<Backend>().is_err());
    }

    #[cfg(feature = "cuda")]
    #[test]
    #[ignore = "requires a working NVIDIA GPU"]
    fn cpu_and_cuda_frames_are_identical() {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/castle-keep-rembg.png");
        let image = SourceImage::open(&fixture).unwrap();
        for columns in [2, 10, 40, 80] {
            let request = RenderRequest::new(&image, columns, RenderOptions::default());
            let cpu = Renderer::new(Backend::Cpu).render(request).unwrap();
            let cuda = Renderer::new(Backend::Cuda).render(request).unwrap();
            assert_eq!(cpu.choices(), cuda.choices(), "mismatch at width {columns}");
            assert_eq!(
                cpu.to_ansi(),
                cuda.to_ansi(),
                "ANSI mismatch at width {columns}"
            );
        }
    }
}
