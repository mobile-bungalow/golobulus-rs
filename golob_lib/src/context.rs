use crate::{ImageFormat, OutDesc, PythonRunner};
use indexmap::IndexMap;
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
    // vector of input numpy arrays, maybe of varying dimensions and integral type
    inputs: IndexMap<String, (PyObject, ImageFormat)>,
    // A single numpy array the same dimensions as the requested frame output
    // this one refers to a pointer over the memory passed in by the user when
    // they started the render pass
    target: PyObject,
    target_width: u32,
    target_height: u32,
    // All registered inputs, types, with default settings and ranges, with a label
    registry: IndexMap<String, crate::Variant>,
    // Context provided time
    time: f32,
    /// A subsection of the output buffer to hand to the user.
    /// If none, it is unconfigured, and we should pass the whole buffer.
    output_size_override: Option<OutputSize>,
    // read only, only true in setup(ctx)
    is_in_setup: bool,
    // The user can set this variable in setup to indicate that this is a continuous effect,
    // which will render on frame after the other, this is only important in after effects
    is_sequential_mode: bool,
    // if set to true in setup all textures passed in will be RGBA order with corrected gamme (i'm
    // looking at you ae 16bit), and all output textures will be translated to their proper image
    // format.
    uses_automatic_color_correction: bool,
    /// numpy helper functions,
    helper_module: Py<PyModule>,
}

#[pymethods]
impl PyContext {
    pub fn get_input(&self, py: Python<'_>, name: &str) -> Option<PyObject> {
        if let Some(v) = self.registry.get(name) {
            match v {
                Variant::Image(_) => self
                    .inputs
                    .get(name)
                    .cloned()
                    .and_then(|t| self.swizzle_to_rgba(py, t.0, t.1).ok())
                    .map(|t| t.to_object(py)),
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

    /// returns height, width pair
    pub fn max_output_size(&self) -> (u32, u32) {
        (self.target_height, self.target_width)
    }

    pub fn output(&self, py: Python) -> Result<PyObject, PyErr> {
        if let Some(OutputSize { width, height }) = self.output_size_override {
            self.helper_module
                .call_method1(py, "center_crop", (&self.target, height, width))
        } else {
            Ok(self.target.clone())
        }
    }

    pub fn set_automatic_color_correction(&mut self, flag: bool) {
        self.uses_automatic_color_correction = flag;
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

    pub fn set_output_size(&mut self, height: u32, width: u32) -> Result<(), PyErr> {
        // The runner needs to be aware of this *regardless* of runner status

        if height == 0 || width == 0 {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Cannot request an output image size with a zero dimension",
            ));
        }

        self.output_size_override = Some(OutputSize { width, height });

        if self.is_in_setup {
            return Ok(());
        }

        if height > self.target_height || width > self.target_width {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "Requested size {height}x{width} exceeds the available {}x{} target image size.",
                self.target_height, self.target_width
            )));
        }

        Ok(())
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

    #[inline(always)]
    fn bail_if_running(&self) -> Result<(), PyErr> {
        if !self.is_in_setup {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "cannot register inputs outside of setup.",
            ));
        }
        Ok(())
    }

    pub fn register_enum(
        &mut self,
        name: &str,
        default: i32,
        map: HashMap<String, i32>,
    ) -> Result<(), PyErr> {
        self.bail_if_running()?;
        let i = Variant::TaggedInt(crate::variant::TaggedInt::new(default, map));
        self.registry.insert(name.to_owned(), i);
        Ok(())
    }

    #[pyo3(signature = (name, min=[-100.0, -100.0], max=[100.0, 100.0], default=[0.0, 0.0]))]
    pub fn register_vector(
        &mut self,
        name: &str,
        min: [f32; 2],
        max: [f32; 2],
        default: [f32; 2],
    ) -> Result<(), PyErr> {
        self.bail_if_running()?;
        let i = Variant::Vector2(Cfg::new(default, min, max));
        self.registry.insert(name.to_owned(), i);
        Ok(())
    }

    #[pyo3(signature = (name, min=-100, max=100, default=0))]
    pub fn register_int(
        &mut self,
        name: &str,
        min: i32,
        max: i32,
        default: i32,
    ) -> Result<(), PyErr> {
        self.bail_if_running()?;
        let i = Variant::Int(Cfg::new(default, min, max));
        self.registry.insert(name.to_owned(), i);
        Ok(())
    }

    #[pyo3(signature = (name, default=false))]
    pub fn register_bool(&mut self, name: &str, default: bool) -> Result<(), PyErr> {
        self.bail_if_running()?;
        let i = Variant::Bool(DiscreteCfg::new(default));
        self.registry.insert(name.to_owned(), i);
        Ok(())
    }

    #[pyo3(signature = (name, default=[1.0, 1.0, 1.0, 1.0]))]
    pub fn register_color(&mut self, name: &str, default: [f32; 4]) -> Result<(), PyErr> {
        self.bail_if_running()?;
        let i = Variant::Color(DiscreteCfg::new(default));
        self.registry.insert(name.to_owned(), i);
        Ok(())
    }

    pub fn register_image_input(&mut self, name: &str) -> Result<(), PyErr> {
        self.bail_if_running()?;
        let i = Variant::Image(DiscreteCfg::new(Image::Input));
        self.registry.insert(name.to_owned(), i);
        Ok(())
    }
}

impl PyContext {
    pub fn new(
        output_descriptor: &OutDesc,
        inputs: IndexMap<String, (PyObject, ImageFormat)>,
        target: PyObject,
        runner: &PythonRunner,
    ) -> Self {
        let registry = if runner.initialized {
            runner.registry.clone()
        } else {
            Default::default()
        };

        Self {
            target_width: output_descriptor.width,
            target_height: output_descriptor.height,
            inputs,
            target,
            registry,
            time: runner.time,
            output_size_override: runner.output_size.clone(),
            is_in_setup: !runner.initialized,
            is_sequential_mode: runner.is_sequential,
            uses_automatic_color_correction: runner.uses_automatic_color_correction,
            helper_module: runner.helper_module.clone(),
        }
    }

    fn swizzle_to_rgba<'a>(
        &'a self,
        py: Python<'a>,
        array: PyObject,
        array_fmt: ImageFormat,
    ) -> Result<PyObject, PyErr> {
        if self.uses_automatic_color_correction
            && matches!(
                array_fmt,
                ImageFormat::Argb16ae | ImageFormat::Argb32 | ImageFormat::Argb8
            )
        {
            self.helper_module.call_method1(py, "rgba_view", (array,))
        } else {
            array.call_method0(py, "view")
        }
    }

    pub fn swizzle_output_to_argb<'a>(&'a self, py: Python<'a>) -> Result<(), PyErr> {
        // FIXME: this is a temporary holdover, numpy is actually slower when doing this
        // swizzle than std::simd swizzle, but I want to stay off nightly for now.
        self.helper_module
            .call_method1(py, "swizzle_in_place", (&self.target,))?;

        Ok(())
    }

    pub fn output_size_requested(&self) -> Option<OutputSize> {
        self.output_size_override.clone()
    }

    pub(crate) fn color_corrected(&self) -> bool {
        self.uses_automatic_color_correction
    }

    pub(crate) fn clone_registry(&self) -> IndexMap<String, crate::Variant> {
        self.registry.clone()
    }
}
