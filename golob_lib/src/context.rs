use crate::ImageFormat;
use indexmap::IndexMap;
use numpy::PyArrayMethods;
use std::collections::HashMap;

use pyo3::prelude::*;

use crate::{
    variant::{Cfg, DiscreteCfg, Image},
    OutputSize, Variant,
};

/// The main context sent to the python script as a global variable.
/// this Allows the user to define inputs, outputs, and properties of
/// the render environment.
#[pyclass]
#[derive(Debug)]
pub struct PyContext {
    fmt: ImageFormat,
    // vector of input numpy arrays, maybe of varying dimensions and integral type
    inputs: IndexMap<String, PyObject>,
    // A single numpy array the same dimensions as the requested frame output
    // this one refers to a pointer over the memory passed in by the user when
    // they started the render pass
    output: PyObject,
    // if the user called `configure_output_size` during a run, we eagerly allocate the new output
    // here, if they already wrote to the other output, well, too bad.
    reallocated_output: Option<PyObject>,
    // All registered inputs, types, with default settings and ranges, with a label
    registry: IndexMap<String, crate::Variant>,
    // Context provided time
    time: f32,
    // which portion of the pixels to hand as output,
    // this should ALWAYS be the max available size unless the user
    // lazily configures it with `configure_output_size`,
    // in that case we allocate an output buffer of the requested size
    // and blit to the real output,
    request_output_size: Option<OutputSize>,
    // only true in setup
    is_in_setup: bool,
    // The user can set this variable in setup to indicate that this is a continuous effect,
    // which will render on frame after the other, this is only important in after effects
    is_sequential_mode: bool,
}

#[pymethods]
impl PyContext {
    pub fn get_input(&self, py: Python<'_>, name: &str) -> Option<PyObject> {
        if let Some(v) = self.registry.get(name) {
            match v {
                Variant::Image(_) => self.inputs.get(name).cloned(),
                Variant::Bool(b) => Some(b.current.into_py(py)),
                Variant::TaggedInt(i) => Some(i.value.into_py(py)),
                Variant::Color(c) => Some(c.current.into_py(py)),
                Variant::Int(i) => Some(i.current.into_py(py)),
                Variant::Float(f) => Some(f.current.into_py(py)),
                Variant::Vector2(v) => Some(v.current.into_py(py)),
            }
        } else {
            None
        }
    }

    pub fn get_output(&self) -> &PyObject {
        if let Some(output) = self.reallocated_output.as_ref() {
            output
        } else {
            &self.output
        }
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn build_info(&self) -> String {
        let profile = if cfg!(debug_assertions) {
            String::from("Debug")
        } else {
            String::from("Release")
        };

        format!("version: {}, {}", env!("CARGO_PKG_VERSION"), profile)
    }

    pub fn configure_output_size(&mut self, py: Python, height: u32, width: u32) {
        match self.fmt {
            ImageFormat::Rgba8 | ImageFormat::Argb8 => {
                self.allocate_proxy_output_texture::<u8>(py, height, width)
            }
            ImageFormat::Argb16ae | ImageFormat::Rgba16 => {
                self.allocate_proxy_output_texture::<u16>(py, height, width)
            }
            ImageFormat::Argb32 | ImageFormat::Rgba32 => {
                self.allocate_proxy_output_texture::<f32>(py, height, width)
            }
        };
    }

    #[pyo3(signature = (name, min=-100.0, max=100.0, default=0.0))]
    pub fn register_float(&mut self, name: &str, min: f32, max: f32, default: f32) {
        let i = Variant::Float(Cfg::new(default, min, max));
        self.registry.insert(name.to_owned(), i);
    }

    pub fn set_sequential_mode(&mut self, is_sequential: bool) -> Result<(), PyErr> {
        if !self.is_in_setup {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Cannot set sequential mode outside of setup",
            ));
        }

        self.is_sequential_mode = is_sequential;
        Ok(())
    }

    pub fn is_sequential_mode(&self) -> bool {
        self.is_sequential_mode
    }

    pub fn register_enum(&mut self, name: &str, default: i32, map: HashMap<String, i32>) {
        let i = Variant::TaggedInt(crate::variant::TaggedInt::new(default, map));
        self.registry.insert(name.to_owned(), i);
    }

    #[pyo3(signature = (name, min=[-100.0, -100.0], max=[100.0, 100.0], default=[0.0, 0.0]))]
    pub fn register_vector(&mut self, name: &str, min: [f32; 2], max: [f32; 2], default: [f32; 2]) {
        let i = Variant::Vector2(Cfg::new(default, min, max));
        self.registry.insert(name.to_owned(), i);
    }

    #[pyo3(signature = (name, min=-100, max=100, default=0))]
    pub fn register_int(&mut self, name: &str, min: i32, max: i32, default: i32) {
        let i = Variant::Int(Cfg::new(default, min, max));
        self.registry.insert(name.to_owned(), i);
    }

    #[pyo3(signature = (name, default=false))]
    pub fn register_bool(&mut self, name: &str, default: bool) {
        let i = Variant::Bool(DiscreteCfg::new(default));
        self.registry.insert(name.to_owned(), i);
    }

    #[pyo3(signature = (name, default=[1.0, 1.0, 1.0, 1.0]))]
    pub fn register_color(&mut self, name: &str, default: [f32; 4]) {
        let i = Variant::Color(DiscreteCfg::new(default));
        self.registry.insert(name.to_owned(), i);
    }

    pub fn register_image_input(&mut self, name: &str) {
        let i = Variant::Image(DiscreteCfg::new(Image::Input));
        self.registry.insert(name.to_owned(), i);
    }
}

impl PyContext {
    pub fn new(
        fmt: crate::ImageFormat,
        inputs: IndexMap<String, PyObject>,
        outputs: PyObject,
        time: f32,
        registry: Option<IndexMap<String, Variant>>,
        is_in_setup: bool,
        is_sequential_mode: bool,
    ) -> Self {
        Self {
            fmt,
            inputs,
            output: outputs,
            registry: registry.unwrap_or_default(),
            reallocated_output: None,
            time,
            request_output_size: None,
            is_in_setup,
            is_sequential_mode,
        }
    }

    fn allocate_proxy_output_texture<T: Copy + Clone + numpy::Element>(
        &mut self,
        py: Python,
        height: u32,
        width: u32,
    ) {
        let dims = if let Ok(out) = self.get_output().downcast_bound::<numpy::PyArray3<T>>(py) {
            out.dims()
        } else {
            // output was None, happens during setup
            self.request_output_size = Some(OutputSize { width, height });
            return;
        };

        if dims[0] != height as usize || dims[1] != width as usize {
            let cur =
                numpy::PyArray3::<T>::zeros_bound(py, [height as usize, width as usize, 4], false)
                    .into_py(py)
                    .into_any();

            self.reallocated_output = Some(cur);
            self.request_output_size = Some(OutputSize { width, height });
        }
    }

    pub fn output_size_requested(&self) -> Option<OutputSize> {
        self.request_output_size.clone()
    }

    pub(crate) fn clone_registry(&self) -> IndexMap<String, crate::Variant> {
        self.registry.clone()
    }
}
