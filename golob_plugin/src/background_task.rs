// contains types and functions for spawning a worker thread.
// to handle sequential background renders. differs from idle_task
// in that it runs on a background thread to not block the UI.

use golob_lib::{PythonRunner, Variant};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;

use crate::footage_utils;

pub type JobId = usize;

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Done,
    Cancelled,
    Busy,
    Ready,
    Error {
        stdout: Option<String>,
        error: String,
    },
}

impl From<golob_lib::GolobulError> for TaskStatus {
    fn from(e: golob_lib::GolobulError) -> Self {
        match e {
            golob_lib::GolobulError::RuntimeError { stderr, stdout } => TaskStatus::Error {
                stdout,
                error: stderr,
            },
            e => TaskStatus::Error {
                stdout: None,
                error: format!("{:?}", e),
            },
        }
    }
}

pub struct ImageBuffer {
    pub name: String,
    pub format: golob_lib::ImageFormat,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

impl ImageBuffer {
    pub fn blank(name: String) -> Self {
        Self {
            name,
            format: golob_lib::ImageFormat::Argb8,
            data: vec![],
            width: 0,
            height: 0,
            stride: 0,
        }
    }
}

pub enum TaskMessage {
    Cancel,
    Job {
        inputs: Vec<(String, Variant)>,
        time: f32,
        frame: u32,
    },
}

pub struct BackgroundTask {
    pub tx: Sender<TaskMessage>,
    pub status: TaskStatus,
    pub buffers: Vec<ImageBuffer>,
}

pub struct OutputDesc {
    pub fmt: golob_lib::ImageFormat,
    pub directory: PathBuf,
    pub last_frame: u32,
    pub width: u32,
    pub height: u32,
}

impl OutputDesc {
    pub fn buffer_len(&self) -> usize {
        self.width as usize * self.height as usize * self.fmt.bytes_per_pixel()
    }
}

impl BackgroundTask {
    pub fn cancel(&self) {
        let _ = self.tx.send(TaskMessage::Cancel);
    }

    pub fn spawn_task(
        id: JobId,
        mut runner: PythonRunner,
        mut desc: OutputDesc,
        task_pool: Arc<dashmap::DashMap<JobId, BackgroundTask>>,
    ) {
        let (tx, rx) = channel();

        let task = BackgroundTask {
            tx,
            status: TaskStatus::Ready,
            buffers: vec![],
        };

        desc.fmt = match desc.fmt {
            golob_lib::ImageFormat::Argb8 => golob_lib::ImageFormat::Rgba8,
            golob_lib::ImageFormat::Argb16ae => golob_lib::ImageFormat::Rgba16,
            golob_lib::ImageFormat::Argb32 => golob_lib::ImageFormat::Rgba32,
            sane => sane,
        };

        task_pool.insert(id, task);

        std::thread::spawn(move || {
            let mut output_buffer = vec![0u8; desc.buffer_len()];
            while let Ok(msg) = rx.recv() {
                match msg {
                    TaskMessage::Job {
                        inputs,
                        time,
                        frame,
                    } => {
                        task_pool.get_mut(&id).unwrap().status = TaskStatus::Busy;

                        for (input_name, input_value) in inputs {
                            let _ = runner.try_set_var(&input_name, input_value);
                        }

                        output_buffer.fill(0);

                        let output = golob_lib::OutDesc {
                            fmt: desc.fmt,
                            width: desc.width,
                            height: desc.height,
                            data: &mut output_buffer,
                            stride: None, // buffer is aligned, no padding
                        };

                        runner.set_time(time);

                        let mut render_pass = runner.create_render_pass(output);
                        let mut task = task_pool.get_mut(&id).unwrap();
                        for layer in task.buffers.iter() {
                            let input = golob_lib::InDesc {
                                fmt: layer.format,
                                width: layer.width,
                                height: layer.height,
                                data: &layer.data,
                                stride: Some(layer.stride),
                            };
                            render_pass.load_input(input, &layer.name);
                        }

                        let res = render_pass.submit();

                        let name =
                            format!("{frame:0pad$}", pad = desc.last_frame.to_string().len());

                        match res {
                            Ok(_) => {
                                // save frame
                                let e = footage_utils::write_image_to_file(
                                    desc.directory.join(name),
                                    &output_buffer,
                                    desc.width,
                                    desc.height,
                                    desc.fmt,
                                );

                                if let Err(e) = e {
                                    log::error!("error while writing file {e}");
                                    task.status = TaskStatus::Error {
                                        stdout: None,
                                        error: format!("{e:?}"),
                                    };
                                    let _ = std::fs::remove_dir_all(desc.directory);
                                    break;
                                }

                                if frame == desc.last_frame {
                                    task.status = TaskStatus::Done;
                                    break;
                                } else {
                                    task.status = TaskStatus::Ready;
                                }
                            }
                            Err(e) => {
                                log::error!("error in Background thread {id}: {e}");
                                task.status = e.into();
                                break;
                            }
                        }
                    }
                    TaskMessage::Cancel => {
                        let directory = &desc.directory;
                        log::debug!("cancelling task, removing directory {directory:?}");
                        let res = std::fs::remove_dir_all(directory);
                        log::debug!("{res:?}");
                        task_pool.get_mut(&id).unwrap().status = TaskStatus::Cancelled;
                        break;
                    }
                }
            }
        });
    }
}
