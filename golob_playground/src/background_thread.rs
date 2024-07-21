use egui::mutex::RwLock;
use golob_lib::{GolobulError, PythonRunner};
use image::imageops::FilterType::Triangle;
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc::Sender, Arc};

#[derive(Debug, Clone)]
pub enum RunnerStatus {
    InitFailed,
    Busy,
    RunFailed,
    NeedsReload(PathBuf),
    Normal { width: usize, height: usize },
}

// This is super disorganized, do this in a
// more principled way when you get a chance
pub struct BgThreadState {
    pub runner: PythonRunner,
    pub dimensions: (usize, usize),
    pub watcher: notify::RecommendedWatcher,
    pub image_inputs: HashMap<String, crate::ImageDesc>,
    pub staging_buffer: Vec<u8>,
    pub current_path: Option<PathBuf>,
    pub filter_mode: egui::TextureFilter,
}

impl BgThreadState {
    pub fn load_script(&mut self, path: &PathBuf) -> Result<Option<String>, GolobulError> {
        log::info!("loading script {path:?}");

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("{e:?}");
                return Err(GolobulError::InvalidModule(format!("{e:?}")));
            }
        };

        if let Some(old) = self.current_path.take() {
            self.watcher.unwatch(&old).unwrap();
        }

        self.watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .unwrap();

        self.current_path = Some(path.clone());

        if let Err(e) = self.runner.clear_script_parent_directory() {
            log::error!("{e:?}");
        }

        if let Some(parent) = path.parent() {
            self.runner.set_script_parent_directory(parent.to_owned());
        }

        let out = self
            .runner
            .load_script(contents, path.to_str().map(|s| s.to_owned()));

        log_run(&out);

        out
    }
    pub fn render(
        &mut self,
        time: f32,
        mut target: egui::TextureHandle,
        status: Arc<RwLock<RunnerStatus>>,
    ) {
        let start = std::time::Instant::now();
        *status.write() = RunnerStatus::Busy;
        self.runner.set_time(time);

        self.staging_buffer.fill(0);

        let o = golob_lib::OutDesc {
            fmt: golob_lib::ImageFormat::Rgba8,
            data: &mut self.staging_buffer,
            height: self.dimensions.0 as u32,
            width: self.dimensions.1 as u32,
            stride: None,
        };

        let mut pass = self.runner.create_render_pass(o);

        for (name, image) in self.image_inputs.iter() {
            let i = golob_lib::InDesc {
                fmt: golob_lib::ImageFormat::Rgba8,
                data: &image.data,
                width: image.width,
                height: image.height,
                stride: None,
            };
            pass.load_input(i, name);
        }

        let out = pass.submit();

        if out.is_err() {
            *status.write() = RunnerStatus::RunFailed;
        }

        log_run(&out);

        let dur = start.elapsed().as_secs_f32();

        log::info!("render took: {dur} secs",);

        if self.runner.requested_output_resize().is_some_and(|size| {
            size.width != self.dimensions.1 as u32 || size.height != self.dimensions.0 as u32
        }) {
            log::debug!("Rerendering with exact buffer specified");
            let size = self.runner.requested_output_resize().unwrap();
            self.staging_buffer = vec![0; (size.width * size.height * 4) as usize];
            self.dimensions.1 = size.width as usize;
            self.dimensions.0 = size.height as usize;
            self.render(time, target, status);
        } else {
            let data = egui::ColorImage::from_rgba_unmultiplied(
                [self.dimensions.1, self.dimensions.0],
                &self.staging_buffer,
            );

            target.set(
                data,
                egui::TextureOptions {
                    magnification: self.filter_mode,
                    minification: self.filter_mode,
                    wrap_mode: egui::TextureWrapMode::ClampToEdge,
                },
            );

            if out.is_err() {
                *status.write() = RunnerStatus::RunFailed;
            } else {
                *status.write() = RunnerStatus::Normal {
                    height: self.dimensions.0,
                    width: self.dimensions.1,
                };
            }
        }
    }
}

pub struct RunnerState {
    pub runner: Arc<RwLock<BgThreadState>>,
    pub status: Arc<RwLock<RunnerStatus>>,
    pub sender: Sender<crate::AppMessage>,
}

pub fn spawn_render_thread(mut target: egui::TextureHandle) -> RunnerState {
    let status_th = Arc::new(RwLock::new(RunnerStatus::Normal {
        height: 255,
        width: 255,
    }));

    let status = status_th.clone();

    let runner = golob_lib::PythonRunner::default();

    let (sender, receiver) = std::sync::mpsc::channel();

    let watcher_clone = sender.clone();
    let watcher = notify::RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                if event.kind.is_modify() {
                    let _ = watcher_clone.send(crate::AppMessage::ReloadScript);
                }
            }
        },
        notify::Config::default(),
    )
    .unwrap();

    let (height, width) = (255, 255);

    let thread_state = BgThreadState {
        watcher,
        runner,
        dimensions: (height, width),
        image_inputs: std::collections::HashMap::new(),
        staging_buffer: vec![0u8; width * height * 4],
        current_path: None,
        filter_mode: egui::TextureFilter::Linear,
    };

    let thread_state = Arc::new(RwLock::new(thread_state));
    let return_runner = thread_state.clone();

    std::thread::spawn(move || {
        let start = std::time::Instant::now();

        while let Ok(msg) = receiver.recv() {
            match msg {
                crate::AppMessage::LoadVenv { path } => {
                    log::info!("loading venv {path:?}");
                    thread_state.write().runner.set_venv_path(path);
                }
                crate::AppMessage::ChangeFilterMode { mode } => {
                    thread_state.write().filter_mode = mode;

                    let data = egui::ColorImage::from_rgba_unmultiplied(
                        [width, height],
                        &thread_state.write().staging_buffer,
                    );

                    target.set(
                        data,
                        egui::TextureOptions {
                            magnification: mode,
                            minification: mode,
                            wrap_mode: egui::TextureWrapMode::ClampToEdge,
                        },
                    );
                }
                crate::AppMessage::UnloadImage { var } => {
                    thread_state.write().image_inputs.remove(&var);

                    thread_state.write().render(
                        start.elapsed().as_secs_f32(),
                        target.clone(),
                        status_th.clone(),
                    );
                }
                crate::AppMessage::LoadImage { var, path } => {
                    let Ok(image) = image::open(&path) else {
                        continue;
                    };

                    let image = image.to_rgba8();
                    let [im_width, im_height] = [image.width(), image.height()];
                    log::info!(
                        "loading image {path:?} with dimensions width : {im_width} height: {im_height}"
                    );

                    let image_buffer = image.into_raw();

                    thread_state.write().image_inputs.insert(
                        var,
                        crate::ImageDesc {
                            data: image_buffer,
                            width: im_width,
                            height: im_height,
                        },
                    );

                    thread_state.write().render(
                        start.elapsed().as_secs_f32(),
                        target.clone(),
                        status_th.clone(),
                    );
                }
                crate::AppMessage::LoadScript { path } => {
                    log::info!("loading script {path:?}");
                    *status_th.write() = RunnerStatus::NeedsReload(path);
                }
                crate::AppMessage::ReloadScript => {
                    let path = thread_state.read().current_path.clone();
                    if let Some(path) = path {
                        let contents = std::fs::read_to_string(path).unwrap();
                        let out = thread_state.write().runner.load_script(contents, None);

                        log_run(&out);

                        if out.is_err() {
                            *status_th.write() = RunnerStatus::InitFailed;
                        } else {
                            *status_th.write() = RunnerStatus::Normal { width, height };
                            thread_state.write().render(
                                start.elapsed().as_secs_f32(),
                                target.clone(),
                                status_th.clone(),
                            );
                        }
                    }
                }
                crate::AppMessage::Render => {
                    thread_state.write().render(
                        start.elapsed().as_secs_f32(),
                        target.clone(),
                        status_th.clone(),
                    );
                }
                crate::AppMessage::ScreenShot { params } => {
                    let home_dir = match homedir::get_my_home() {
                        Ok(Some(home)) => home,
                        _ => "/".into(),
                    };

                    let cur = thread_state.read().current_path.clone();
                    let home_dir = cur.as_ref().and_then(|p| p.parent()).unwrap_or(&home_dir);

                    let Some(file) = rfd::FileDialog::new()
                        .set_directory(home_dir)
                        .set_file_name("screenshot.png")
                        .save_file()
                    else {
                        continue;
                    };

                    let mut file = file;

                    let mut image = image::RgbaImage::from_raw(
                        width as u32,
                        height as u32,
                        thread_state.read().staging_buffer.clone(),
                    )
                    .unwrap();

                    if let Some((width, height, filter)) = params {
                        let filter = match filter {
                            egui::TextureFilter::Nearest => image::imageops::FilterType::Nearest,
                            egui::TextureFilter::Linear => Triangle,
                        };

                        image = image::imageops::resize(&image, width, height, filter);
                    }

                    if file.extension().is_none() {
                        file.set_extension("png");
                    }

                    image.save(file).unwrap();
                }
            }
        }
    });

    RunnerState {
        status,
        sender,
        runner: return_runner,
    }
}

fn log_run(res: &std::result::Result<Option<String>, golob_lib::GolobulError>) {
    match res {
        Ok(Some(out)) => {
            log::info!("{}", out.trim_end());
        }
        Err(e) => {
            if let golob_lib::GolobulError::RuntimeError { stderr, stdout } = e {
                if let Some(stdout) = stdout {
                    log::info!("{}", stdout.trim_end());
                }
                log::error!("{stderr:?}");
            } else {
                log::error!("{e:?}");
            }
        }
        _ => {}
    };
}
