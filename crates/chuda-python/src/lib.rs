use std::path::PathBuf;

use chuda_core::{Backend, RenderOptions, RenderRequest};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
    types::{PyBytes, PyModule},
};

#[pyclass(name = "Image", module = "chuda")]
struct PyImage {
    inner: chuda_core::SourceImage,
}

#[pymethods]
impl PyImage {
    #[staticmethod]
    fn open(path: PathBuf) -> PyResult<Self> {
        Ok(Self {
            inner: chuda_core::SourceImage::open(&path).map_err(value_error)?,
        })
    }
    #[staticmethod]
    fn from_png(py: Python<'_>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self {
            inner: chuda_core::SourceImage::from_png(&buffer_bytes(py, data)?)
                .map_err(value_error)?,
        })
    }
    #[staticmethod]
    fn from_rgba(
        py: Python<'_>,
        width: u32,
        height: u32,
        data: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        Ok(Self {
            inner: chuda_core::SourceImage::from_raw(width, height, buffer_bytes(py, data)?)
                .map_err(value_error)?,
        })
    }
    #[getter]
    fn width(&self) -> u32 {
        self.inner.width()
    }
    #[getter]
    fn height(&self) -> u32 {
        self.inner.height()
    }
}

#[pyclass(name = "Frame", module = "chuda", frozen)]
struct PyFrame {
    inner: chuda_core::Frame,
}

#[pymethods]
impl PyFrame {
    #[getter]
    fn columns(&self) -> u32 {
        self.inner.columns()
    }
    #[getter]
    fn rows(&self) -> u32 {
        self.inner.rows()
    }
    #[getter]
    fn backend(&self) -> String {
        self.inner.backend().to_string()
    }
    #[getter]
    fn glyphs(&self) -> Vec<u32> {
        self.inner
            .choices()
            .iter()
            .map(|cell| cell.codepoint)
            .collect()
    }
    #[getter]
    fn foreground<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(
            py,
            &self
                .inner
                .choices()
                .iter()
                .flat_map(|cell| cell.fg)
                .collect::<Vec<_>>(),
        )
    }
    #[getter]
    fn background<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(
            py,
            &self
                .inner
                .choices()
                .iter()
                .flat_map(|cell| cell.bg)
                .collect::<Vec<_>>(),
        )
    }
    #[getter]
    fn background_transparent<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(
            py,
            &self
                .inner
                .choices()
                .iter()
                .map(|cell| cell.transparent_bg)
                .collect::<Vec<_>>(),
        )
    }
    fn to_ansi<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.inner.to_ansi())
    }
}

#[pyclass(name = "Renderer", module = "chuda")]
struct PyRenderer {
    inner: chuda_core::Renderer,
}

#[pymethods]
impl PyRenderer {
    #[new]
    #[pyo3(signature = (backend = "auto", max_batch_cells = 262_144))]
    fn new(backend: &str, max_batch_cells: usize) -> PyResult<Self> {
        let backend = backend.parse::<Backend>().map_err(value_error)?;
        Ok(Self {
            inner: chuda_core::Renderer::with_batch_limit(backend, max_batch_cells),
        })
    }

    #[pyo3(signature = (image, columns, font_ratio = 2.0, transparent_threshold = 0.10))]
    fn render(
        &self,
        py: Python<'_>,
        image: &PyImage,
        columns: u32,
        font_ratio: f32,
        transparent_threshold: f32,
    ) -> PyResult<PyFrame> {
        let options = RenderOptions {
            font_ratio,
            transparent_threshold,
        };
        let frame = py
            .detach(|| {
                self.inner
                    .render(RenderRequest::new(&image.inner, columns, options))
            })
            .map_err(runtime_error)?;
        emit_warning(py, &self.inner)?;
        Ok(PyFrame { inner: frame })
    }

    #[pyo3(signature = (requests, font_ratio = 2.0, transparent_threshold = 0.10))]
    fn render_many(
        &self,
        py: Python<'_>,
        requests: Vec<(Py<PyImage>, u32)>,
        font_ratio: f32,
        transparent_threshold: f32,
    ) -> PyResult<Vec<PyFrame>> {
        let options = RenderOptions {
            font_ratio,
            transparent_threshold,
        };
        let borrowed: Vec<_> = requests
            .iter()
            .map(|(image, columns)| (image.borrow(py), *columns))
            .collect();
        let native: Vec<_> = borrowed
            .iter()
            .map(|(image, columns)| RenderRequest::new(&image.inner, *columns, options))
            .collect();
        let frames = py
            .detach(|| self.inner.render_many(&native))
            .map_err(runtime_error)?;
        emit_warning(py, &self.inner)?;
        Ok(frames.into_iter().map(|inner| PyFrame { inner }).collect())
    }
}

fn emit_warning(py: Python<'_>, renderer: &chuda_core::Renderer) -> PyResult<()> {
    if let Some(message) = renderer.take_fallback_warning() {
        py.import("logging")?
            .call_method1("getLogger", ("chuda",))?
            .call_method1("warning", (message,))?;
    }
    Ok(())
}

fn buffer_bytes(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    py.import("builtins")?
        .getattr("memoryview")?
        .call1((value,))?
        .call_method0("tobytes")?
        .extract()
}

fn value_error(error: anyhow::Error) -> PyErr {
    PyValueError::new_err(error.to_string())
}
fn runtime_error(error: anyhow::Error) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}

#[pymodule]
fn chuda(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add_class::<PyImage>()?;
    module.add_class::<PyFrame>()?;
    module.add_class::<PyRenderer>()?;
    Ok(())
}
