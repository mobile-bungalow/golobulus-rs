use pyo3::prelude::*;
use pyo3::{IntoPy, Py, PyAny, Python};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::thread;

static PYTHON_EVENT_LOOP: OnceLock<Py<PyAny>> = OnceLock::new();

pub fn get_event_loop() -> PyObject {
    PYTHON_EVENT_LOOP
        .get_or_init(|| {
            let event_loop = Python::with_gil(|py| {
                let asyncio = py
                    .import_bound("asyncio")
                    .expect("Failed to import asyncio");

                let loop_ = asyncio
                    .call_method0("new_event_loop")
                    .expect("Failed to create new event loop");

                asyncio
                    .call_method1("set_event_loop", (&loop_,))
                    .expect("Failed to set event loop");

                loop_.into_py(py)
            });

            let clone = event_loop.clone();
            thread::spawn(move || {
                Python::with_gil(|py| {
                    clone
                        .call_method0(py, "run_forever")
                        .expect("Failed to run event loop");
                });
            });

            event_loop
        })
        .clone()
}

use pyo3::create_exception;
use pyo3::exceptions::PyException;

create_exception!(golobulus, SendError, PyException, "Rust Channel Error: ");

#[pyclass]
pub struct RustChan {
    tx: Sender<PyResult<Py<PyAny>>>,
}

#[pymethods]
impl RustChan {
    pub fn __call__(&self, py: Python, result: Py<PyAny>) {
        let res = result.call_method0(py, "result");
        let _ = self.tx.send(res);
    }
}

impl RustChan {
    pub fn new() -> (Self, Receiver<PyResult<Py<PyAny>>>) {
        let (tx, rx) = channel();
        (Self { tx }, rx)
    }
}
