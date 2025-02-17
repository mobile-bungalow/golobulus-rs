use errors::{traceback, StdOutCatcher};
use pyo3::types::IntoPyDict;
pub mod context;
mod errors;
pub mod event_loop;
mod variant;

use indexmap::IndexMap;
use numpy::{npyffi, PY_ARRAY_API};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
};

pub use errors::GolobulError;

use pyo3::{
    prelude::*,
    types::{PyFunction, PyModule},
};

pub use variant::{Cfg, DiscreteCfg, Image, Variant};

/// A list of supported image formats, using varying inputs and outputs
/// may require additional copies and casting.
#[derive(Debug, Copy, Clone)]
pub enum ImageFormat {
    Rgba8,
    Argb8,
    Argb16ae,
    Rgba16,
    Argb32,
    Rgba32,
}

// allows python results to be polled outside of
// the GIL
enum MaybeFuture {
    Done(Option<String>),
    Channel(
        Receiver<PyResult<Py<PyAny>>>,
        Py<context::PyContext>,
        Py<StdOutCatcher>,
    ),
}

impl ImageFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            ImageFormat::Rgba8 | ImageFormat::Argb8 => 4,
            ImageFormat::Argb16ae | ImageFormat::Rgba16 => 8,
            ImageFormat::Argb32 | ImageFormat::Rgba32 => 16,
        }
    }
}

/// A borrowed view into an image stored in memory
#[derive(Debug)]
pub struct InDesc<'a> {
    pub fmt: ImageFormat,
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    // buffer stride if it's not just the width
    pub stride: Option<u32>,
}

/// A mutable borrowed view into an image stored in memory
#[derive(Debug)]
pub struct OutDesc<'a> {
    pub fmt: ImageFormat,
    pub data: &'a mut [u8],
    pub width: u32,
    pub height: u32,
    // stride if it's not just the width
    pub stride: Option<u32>,
}

impl<'a> OutDesc<'a> {
    pub fn empty() -> Self {
        Self {
            fmt: ImageFormat::Rgba8,
            data: &mut [],
            width: 0,
            height: 0,
            stride: None,
        }
    }
    pub fn is_well_structured(&self) -> Result<(), GolobulError> {
        let bytes_per_pixel = self.fmt.bytes_per_pixel();
        let row_size = self.width as usize * bytes_per_pixel;
        let padded_row_size = self.stride.unwrap_or(row_size as u32) as usize;
        let expected_data_size = padded_row_size * self.height as usize;

        if self.width == 0 || self.height == 0 {
            return Err(GolobulError::ZeroDimension);
        }

        if self.data.len() != expected_data_size {
            return Err(GolobulError::SizeMismatch {
                expected: expected_data_size,
                found: self.data.len(),
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct PythonRunner {
    script_module_uuid: String,
    /// The main script module loaded by the user.
    script_module: Py<PyModule>,
    /// Numpy helper
    helper_module: Py<PyModule>,
    /// Global python asyncio event loop running on a background thread, on windows this
    /// must be initialized form the main thread.
    event_loop: Py<PyAny>,
    /// Registry of all the inputs
    registry: IndexMap<String, Variant>,
    /// Time that can be set by the user, single special case float input
    time: f32,
    /// The user requested outputsize that we will do out best to respect
    output_size: Option<OutputSize>,
    /// Should be named `sites_packages_path`, another module search path.
    pyenv_path: Option<PathBuf>,
    /// Additionaly module search path
    script_parent_directory: Option<PathBuf>,
    /// If true then AE instances will have the option to render on a BG thread
    /// serially.
    is_sequential: bool,
    /// if true, we call finalization the output arrays and swizzle the views into the input
    /// arrays.
    uses_automatic_color_correction: bool,
    /// If true, setup has run successfully
    initialized: bool,
}

const DEFAULT_SCRIPT: &str = r"
def setup(ctx):
    ctx.register_image_input('input')
    pass

def run(ctx):
    input = ctx.get_input('input')

    if input is None:
        output = ctx.output()
        output.fill(0)
        return

    ctx.set_output_size(input.shape[0], input.shape[1])
    output = ctx.output()
    output[:] = input
    pass
";

impl Default for PythonRunner {
    fn default() -> Self {
        Self::new(DEFAULT_SCRIPT.to_owned(), Some("default.py".to_owned())).unwrap()
    }
}

pub struct RenderPass<'a> {
    runner: &'a mut PythonRunner,
    inputs: IndexMap<String, InDesc<'a>>,
    output: OutDesc<'a>,
}

impl<'a> RenderPass<'a> {
    pub fn submit(self) -> Result<Option<String>, GolobulError> {
        let Self {
            runner,
            inputs,
            output,
        } = self;

        runner.run(inputs, output)
    }

    pub fn load_input(&mut self, input: InDesc<'a>, name: &str) {
        self.inputs.insert(name.to_owned(), input);
    }
}

impl PythonRunner {
    fn new(src: String, file_name: Option<String>) -> Result<Self, GolobulError> {
        pyo3::prepare_freethreaded_python();

        let event_loop = event_loop::get_event_loop();

        let helper_module: Py<PyModule> = Python::with_gil(|py| {
            #[cfg(target_os = "macos")]
            if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
                let version_info = py.version_info();
                let sys = py.import_bound("sys").unwrap();
                let sys_path = sys.getattr("path").unwrap();

                sys_path
                    .call_method1(
                        "append",
                        (format!(
                            "{}/lib/python{}.{}/site-packages",
                            venv, version_info.major, version_info.minor
                        ),),
                    )
                    .unwrap();
            }

            let asyncio = py
                .import_bound("asyncio")
                .expect("Failed to import asyncio");

            asyncio
                .call_method1("set_event_loop", (&event_loop.clone(),))
                .expect("Failed to set event loop");

            let module = PyModule::from_code_bound(
                py,
                include_str!("./numpy_helper.py"),
                "helper.py",
                "helper",
            )
            .map_err(|e| {
                e.display(py);
                GolobulError::InvalidModule(format!("{e:?}"))
            })?;

            Ok::<_, GolobulError>(module.into())
        })?;

        let (uuid, script_module) = load_module(src, file_name)?;

        let mut out = PythonRunner {
            script_module_uuid: uuid.into(),
            helper_module,
            event_loop,
            script_module,
            registry: IndexMap::new(),
            time: 0.,
            output_size: None,
            pyenv_path: None,
            script_parent_directory: None,
            is_sequential: false,
            uses_automatic_color_correction: true,
            initialized: false,
        };

        out.setup()?;

        Ok(out)
    }

    fn delete_module(&self) -> Result<(), PyErr> {
        Python::with_gil(|py| {
            let sys = py.import_bound("sys")?;

            let mods = sys.getattr("modules")?;
            let modules: &Bound<pyo3::types::PyDict> = mods.downcast()?;

            let module_name = self.script_module_uuid.clone();
            if modules.contains(&module_name)? {
                modules.del_item(&module_name)?;
            }

            Ok(())
        })
    }

    /// Loads the given python script. returning stdout if it appeared
    pub fn load_script<S: AsRef<str>>(
        &mut self,
        src: S,
        file_name: Option<String>,
    ) -> Result<Option<String>, GolobulError> {
        if let Some(pyenv_path) = self.pyenv_path.as_ref() {
            self.add_path_to_sys(&pyenv_path.clone())?;
        }

        if let Some(script_dir) = self.script_parent_directory.as_ref() {
            self.add_path_to_sys(&script_dir.clone())?;
        }

        self.delete_module()
            .map_err(|_| GolobulError::CastingError)?;

        let (uuid, new_mod) = load_module(src, file_name)?;

        self.script_module_uuid = uuid.into();
        self.script_module = new_mod;
        self.initialized = false;

        self.setup()
    }

    /// This sets the venve path for *the next time*
    /// the runner loads a new script
    pub fn set_venv_path(&mut self, path: PathBuf) {
        let _ = self.add_path_to_sys(&path);
        self.pyenv_path = Some(path);
    }

    pub fn clear_venv_path(&mut self) -> Result<(), GolobulError> {
        if let Some(pyenv_path) = self.pyenv_path.take() {
            self.remove_path_from_sys(&pyenv_path)?;
        }
        Ok(())
    }

    /// This sets the script parent dir for *the next time*
    /// the runner loads a new script
    pub fn set_script_parent_directory(&mut self, path: PathBuf) {
        self.script_parent_directory = Some(path);
    }

    pub fn clear_script_parent_directory(&mut self) -> Result<(), GolobulError> {
        if let Some(pyenv_path) = self.script_parent_directory.take() {
            self.remove_path_from_sys(&pyenv_path)?;
        }
        Ok(())
    }

    /// If  this is Some(size) it represent the exepcted dimension
    /// of outputs passed into the renderpass.
    /// If you do not respect this you will incur
    /// an entire allocation on each `run` call.
    /// if it's None, the user is saying they are okay with any output
    pub fn requested_output_resize(&self) -> Option<OutputSize> {
        self.output_size.clone()
    }

    /// Returns true if the script set "is_sequential"
    /// in setup.
    pub fn is_sequential(&self) -> bool {
        self.is_sequential
    }

    /// Attemp to set a variable, returns an error if missing or if htere is a type mismatch.
    pub fn try_set_var(&mut self, name: &str, value: Variant) -> Result<(), GolobulError> {
        if let Some(entry) = self.registry.get_mut(name) {
            entry.adopt(&value)?;
            Ok(())
        } else {
            Err(GolobulError::MissingVar(name.to_owned()))
        }
    }

    // Runs then returns the contents of stdout if it exists
    fn run(
        &mut self,
        inputs: IndexMap<String, InDesc>,
        mut output: OutDesc,
    ) -> Result<Option<String>, GolobulError> {
        output.is_well_structured()?;

        let result = Python::with_gil(|py| -> Result<MaybeFuture, GolobulError> {
            let out_catcher = StdOutCatcher::new(py)?;

            let inputs = inputs
                .iter()
                .map(|(k, v)| {
                    let view = slice_view(v, &py);
                    (k.clone(), (view.into_py(py), v.fmt))
                })
                .collect();

            let target_image = mutable_slice_view(&mut output, &py);

            let ctx = context::PyContext::new(&output, inputs, target_image.into_py(py), self);

            let ctx = Py::new(py, ctx).map_err(|_| GolobulError::BoundError)?;

            let maybe_future = self
                .script_module
                .call_method1(py, "run", (&ctx,))
                .map_err(|e| traceback(e, &out_catcher, py));

            // Throw recoverable error if descriptor was bad.
            if maybe_future.is_err()
                && ctx
                    .borrow(py)
                    .output_size_requested()
                    .is_some_and(|size| size.width > output.width || size.height > output.height)
            {
                self.output_size = ctx.borrow(py).output_size_requested();

                let s = self.output_size.clone().unwrap();

                return Err(GolobulError::OutputSizeTooLarge {
                    req: (s.height, s.width),
                    avail: (output.height, output.width),
                });
            }

            let maybe_future = maybe_future?;

            if is_awaitable(py, &maybe_future).unwrap() {
                let asio = py.import_bound("asyncio").map_err(|_| GolobulError::Asio)?;

                let (py_chan, rust_chan) = event_loop::RustChan::new();

                let res = asio
                    .call_method(
                        "run_coroutine_threadsafe",
                        (maybe_future, self.event_loop.bind(py)),
                        None,
                    )
                    .map_err(|_| GolobulError::Asio)?;

                res.call_method1("add_done_callback", (py_chan,))
                    .map_err(|_| GolobulError::Asio)?;

                Ok(MaybeFuture::Channel(rust_chan, ctx, out_catcher))
            } else {
                let out = self.finalize(&ctx, &py, &mut output, &out_catcher)?;
                Ok(MaybeFuture::Done(out))
            }
        })?;

        match result {
            MaybeFuture::Done(result) => Ok(result),
            MaybeFuture::Channel(rx, ctx, out_catcher) => match rx.recv() {
                Ok(Ok(_)) => {
                    Python::with_gil(|py| self.finalize(&ctx, &py, &mut output, &out_catcher))
                }
                Ok(Err(e)) => Err(Python::with_gil(|py| {
                    if ctx.borrow(py).output_size_requested().is_some_and(|size| {
                        size.width > output.width || size.height > output.height
                    }) {
                        self.output_size = ctx.borrow(py).output_size_requested();

                        let s = self.output_size.clone().unwrap();

                        return GolobulError::OutputSizeTooLarge {
                            req: (s.height, s.width),
                            avail: (output.height, output.width),
                        };
                    } else {
                        traceback(e, &out_catcher, py)
                    }
                })),
                Err(_) => Err(GolobulError::Asio),
            },
        }
    }

    fn finalize(
        &mut self,
        ctx: &Py<context::PyContext>,
        py: &Python,
        output: &mut OutDesc,
        out_catcher: &Py<StdOutCatcher>,
    ) -> Result<Option<String>, GolobulError> {
        let ctx_ref = ctx.borrow(*py);

        if matches!(
            output.fmt,
            ImageFormat::Argb8 | ImageFormat::Argb32 | ImageFormat::Argb16ae
        ) {
            ctx_ref.swizzle_output_to_argb(*py).unwrap();
        }

        if let Some(size) = ctx_ref.output_size_requested() {
            self.output_size = Some(size);
        }

        let out = out_catcher.borrow_mut(*py).output.take();
        Ok(out)
    }

    // runs setup, returning stdout
    fn setup(&mut self) -> Result<Option<String>, GolobulError> {
        // point at the current pyenv
        Python::with_gil(|py| {
            // we need to use the numpy API safely ONCE before
            // trying to use it unsafely
            py.import_bound("numpy")
                .map_err(|_e| GolobulError::InvalidModule("You couldn't lock numpy!".to_owned()))?;

            // Torch additionally wants to be loaded on the main thread, once.
            // This might fail but it prevents segfaults based on crappy assumptions in the
            // pytorch code base.
            let _ = py.import_bound("torch");

            let out_catcher =
                Bound::new(py, StdOutCatcher::default()).map_err(|_| GolobulError::BoundError)?;

            let sys = py
                .import_bound("sys")
                .map_err(|_| GolobulError::BoundError)?;

            sys.setattr("stdout", &out_catcher)
                .map_err(|_| GolobulError::InvalidModule("Could not set stdout".to_owned()))?;

            let ctx = context::PyContext::new(
                &OutDesc::empty(),
                Default::default(),
                ().into_py(py),
                self,
            );

            let ctx = Bound::new(py, ctx).map_err(|_| GolobulError::BoundError)?;

            self.script_module
                .call_method1(py, "setup", (&ctx,))
                .map_err(|e| {
                    let stdout = out_catcher.borrow_mut().output.take();
                    GolobulError::RuntimeError {
                        stderr: format!("{e}"),
                        stdout,
                    }
                })?;

            let mut registry = ctx.borrow().clone_registry();

            for (k, v) in registry.iter_mut() {
                if let Some(entry) = self.registry.get_mut(k) {
                    let _ = v.adopt(entry);
                }
            }

            self.output_size = ctx.borrow().output_size_requested();
            self.is_sequential = ctx.borrow().is_sequential_mode();
            self.uses_automatic_color_correction = ctx.borrow().color_corrected();
            self.registry = registry;
            self.initialized = true;

            let out = out_catcher.borrow_mut().output.take();
            Ok(out)
        })
    }

    pub fn set_time(&mut self, time: f32) {
        self.time = time;
    }

    pub fn create_render_pass<'a>(&'a mut self, output: OutDesc<'a>) -> RenderPass<'a> {
        RenderPass {
            runner: self,
            inputs: Default::default(),
            output,
        }
    }

    fn remove_path_from_sys(&mut self, target_path: &Path) -> Result<(), GolobulError> {
        Python::with_gil(|py| -> PyResult<()> {
            let sys = py.import_bound("sys")?;
            let path = sys.getattr("path")?;
            path.call_method1("remove", (target_path.into_py(py).into_bound(py),))?;
            Ok(())
        })
        .map_err(|_| GolobulError::PathUpdateError)
    }

    fn add_path_to_sys(&mut self, new_path: &Path) -> Result<(), GolobulError> {
        Python::with_gil(|py| -> PyResult<()> {
            let sys = py.import_bound("sys")?;
            let path = sys.getattr("path")?;
            let dict = [
                ("path", path),
                ("new_item", new_path.into_py(py).into_bound(py)),
            ]
            .into_py_dict_bound(py);

            py.eval_bound(
                "path.insert(0, new_item) if new_item not in path else None",
                None,
                Some(&dict),
            )?;

            Ok(())
        })
        .map_err(|_| GolobulError::PathUpdateError)
    }

    pub fn iter_inputs(&self) -> impl Iterator<Item = (&String, &Variant)> {
        self.registry.iter()
    }

    pub fn iter_inputs_mut(&mut self) -> impl Iterator<Item = (&String, &mut Variant)> {
        self.registry.iter_mut()
    }
}

fn load_module<S: AsRef<str>>(
    src: S,
    file_name: Option<String>,
) -> Result<(uuid::Uuid, Py<PyModule>), GolobulError> {
    let uuid = uuid::Uuid::new_v4();
    Python::with_gil(|py| {
        let module = PyModule::from_code_bound(
            py,
            src.as_ref(),
            &file_name.unwrap_or_default(),
            &uuid.simple().to_string(),
        )
        .map_err(|e| {
            e.display(py);
            GolobulError::InvalidModule(format!("{e:?}"))
        })?;

        // This is as close as we can get
        // to a real type check. can't check the
        // arguments or return type.
        match module.getattr("run") {
            Ok(run) => {
                if !run.is_instance_of::<PyFunction>() {
                    return Err(GolobulError::MissingRun);
                }
            }
            Err(_) => return Err(GolobulError::MissingRun),
        }

        match module.getattr("setup") {
            Ok(setup) => {
                if !setup.is_instance_of::<PyFunction>() {
                    return Err(GolobulError::MissingSetup);
                }
            }
            Err(_) => return Err(GolobulError::MissingSetup),
        }

        Ok((uuid, module.into()))
    })
}

/// Build a mutable slice view
fn mutable_slice_view<'a>(out_desc: &mut OutDesc, py: &'a Python) -> pyo3::Bound<'a, PyAny> {
    let OutDesc {
        fmt,
        data,
        width,
        height,
        stride,
    } = out_desc;

    let bytes_per_pixel = fmt.bytes_per_pixel();

    let mut dims = [*height as isize, *width as isize, 4];

    let mut stride = [
        stride.unwrap_or({ *width } * bytes_per_pixel as u32) as isize,
        bytes_per_pixel as isize,
        bytes_per_pixel as isize / 4,
    ];

    let ty = match fmt {
        ImageFormat::Rgba8 | ImageFormat::Argb8 => npyffi::types::NPY_TYPES::NPY_UBYTE,
        ImageFormat::Rgba16 | ImageFormat::Argb16ae => npyffi::types::NPY_TYPES::NPY_USHORT,
        ImageFormat::Argb32 | ImageFormat::Rgba32 => npyffi::types::NPY_TYPES::NPY_FLOAT,
    };

    let flags = npyffi::flags::NPY_ARRAY_C_CONTIGUOUS | npyffi::NPY_ARRAY_WRITEABLE;

    unsafe {
        let pyarray_ptr = PY_ARRAY_API.PyArray_New(
            *py,
            PY_ARRAY_API.get_type_object(*py, npyffi::NpyTypes::PyArray_Type) as *mut _,
            3,
            dims.as_mut_ptr(),
            ty as i32,
            stride.as_mut_ptr(),
            data.as_mut_ptr() as *mut _,
            0, // itemsize: gets ignored if ty is an integral type
            flags,
            std::ptr::null_mut(),
        );

        Bound::from_borrowed_ptr(*py, pyarray_ptr)
    }
}

/// Build a slice view
fn slice_view<'a>(in_desc: &InDesc, py: &'a Python) -> pyo3::Bound<'a, PyAny> {
    let InDesc {
        fmt,
        data,
        width,
        height,
        stride,
    } = in_desc;

    let bytes_per_pixel = fmt.bytes_per_pixel();

    let mut dims = [*height as isize, *width as isize, 4];

    let mut stride = [
        stride.unwrap_or(*width * bytes_per_pixel as u32) as isize,
        bytes_per_pixel as isize,
        bytes_per_pixel as isize / 4,
    ];

    let ty = match fmt {
        ImageFormat::Rgba8 | ImageFormat::Argb8 => npyffi::types::NPY_TYPES::NPY_UBYTE,
        ImageFormat::Rgba16 | ImageFormat::Argb16ae => npyffi::types::NPY_TYPES::NPY_USHORT,
        ImageFormat::Argb32 | ImageFormat::Rgba32 => npyffi::types::NPY_TYPES::NPY_FLOAT,
    };

    let flags = npyffi::flags::NPY_ARRAY_C_CONTIGUOUS;

    unsafe {
        let py_array_slice = PY_ARRAY_API.PyArray_New(
            *py,
            PY_ARRAY_API.get_type_object(*py, npyffi::NpyTypes::PyArray_Type) as *mut _,
            3,
            dims.as_mut_ptr(),
            ty as i32,
            stride.as_mut_ptr(),
            data.as_ptr() as *mut _,
            0,
            flags,
            std::ptr::null_mut(),
        );

        Bound::from_borrowed_ptr(*py, py_array_slice)
    }
}

fn is_awaitable(py: Python, obj: &Py<PyAny>) -> PyResult<bool> {
    let inspect = py.import_bound("inspect")?;
    let is_awaitable = inspect.getattr("isawaitable")?;
    is_awaitable.call1((obj,))?.extract()
}

pub fn update_dylib_search_path(path: &str) -> Result<(), GolobulError> {
    pyo3::prepare_freethreaded_python();
    Python::with_gil(|py| {
        let module = py
            .import_bound("os")
            .map_err(|_| GolobulError::DllSearchError)?;

        module
            .call_method1("add_dll_directory", (path,))
            .map_err(|_| GolobulError::DllSearchError)?;

        Ok(())
    })
}
