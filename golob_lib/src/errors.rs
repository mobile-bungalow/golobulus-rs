use pyo3::prelude::*;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GolobulError {
    #[error("Error updating dll path, cannot start.")]
    DllSearchError,
    #[error("Invalid Module: {0}")]
    InvalidModule(String),
    #[error("The loaded module was missing the `setup()` function")]
    MissingSetup,
    #[error("The loaded module was missing the `run()` function")]
    MissingRun,
    #[error("runtime error: {stderr} \n stdout content: {stdout:?}")]
    RuntimeError {
        stderr: String,
        stdout: Option<String>,
    },
    #[error("invalid buffer size (expected {expected:?}, found {found:?})")]
    SizeMismatch { expected: usize, found: usize },
    #[error("Output buffer passed with 0 height or width!")]
    ZeroDimension,
    #[error("Error Casting py type")]
    CastingError,
    #[error("Tried to get slice from noncontiguous numpy array")]
    Noncontiguous(#[from] numpy::NotContiguousError),
    #[error("Could not update pypath")]
    PathUpdateError,
    #[error("Could not create bound variable")]
    BoundError,
    #[error("Error running async code")]
    Asio,
    #[error("Type mismatch while setting variable.")]
    TypeMismatch,
    #[error("No Input {0} found")]
    MissingVar(String)
}

#[pyclass]
#[derive(Default)]
pub struct StdOutCatcher {
    pub output: Option<String>,
}

#[pymethods]
impl StdOutCatcher {
    fn write(&mut self, data: &str) {
        if let Some(out) = self.output.as_mut() {
            out.push_str(data);
        } else {
            self.output = Some(data.to_owned())
        }
    }
}
