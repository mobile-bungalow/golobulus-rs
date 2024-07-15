use egui::mutex::RwLock;
use golob_lib::PythonRunner;
use notify::*;
use std::collections::HashMap;
use std::sync::{mpsc::Sender, Arc};

#[derive(Debug, Clone)]
pub enum RunnerStatus {
    InitFailed,
    Busy,
    RunFailed,
    Normal { width: usize, height: usize },
}

pub struct RunnerState {
    pub runner: Arc<RwLock<PythonRunner>>,
    pub status: Arc<RwLock<RunnerStatus>>,
    pub sender: Sender<crate::AppMessage>,
}

fn render(
    time: f32,
    width: &mut usize,
    height: &mut usize,
    mut buf: &mut Vec<u8>,
    runner: &mut PythonRunner,
    image_inputs: &HashMap<String, crate::ImageDesc>,
    mut target: egui::TextureHandle,
    status: Arc<RwLock<RunnerStatus>>,
) {
    let start = std::time::Instant::now();
    *status.write() = RunnerStatus::Busy;
    runner.set_time(time);

    buf.fill(0);
    let o = golob_lib::OutDesc {
        fmt: golob_lib::ImageFormat::Rgba8,
        data: &mut buf,
        height: *height as u32,
        width: *width as u32,
        stride: None,
    };

    let mut pass = runner.create_render_pass(o);

    for (name, image) in image_inputs.iter() {
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
    log::info!("render took: {dur} secs");

    if runner
        .requested_output_resize()
        .is_some_and(|size| size.width != *width as u32 || size.height != *height as u32)
    {
        let size = runner.requested_output_resize().unwrap();
        *buf = vec![0; (size.width * size.height * 4) as usize];
        *width = size.width as usize;
        *height = size.height as usize;
        render(
            time,
            width,
            height,
            buf,
            runner,
            image_inputs,
            target,
            status,
        );
    } else {
        let data = egui::ColorImage::from_rgba_unmultiplied([*width, *height], buf);
        target.set(data, Default::default());
        *status.write() = RunnerStatus::Normal {
            height: *height,
            width: *width,
        };
    }
}

pub fn spawn_render_thread(target: egui::TextureHandle) -> RunnerState {
    let status_th = Arc::new(RwLock::new(RunnerStatus::Normal {
        height: 255,
        width: 255,
    }));
    let status = status_th.clone();

    let runner_th = Arc::new(RwLock::new(golob_lib::PythonRunner::default()));
    let runner = runner_th.clone();

    let (sender, receiver) = std::sync::mpsc::channel();

    let watcher_clone = sender.clone();
    let mut watcher = notify::RecommendedWatcher::new(
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

    std::thread::spawn(move || {
        let (mut width, mut height) = (255, 255);
        let mut staging_buffer = vec![0u8; width * height * 4];
        let mut image_inputs = std::collections::HashMap::new();
        let mut current_path: Option<std::path::PathBuf> = None;
        let start = std::time::Instant::now();

        while let Ok(msg) = receiver.recv() {
            match msg {
                crate::AppMessage::UnloadImage { var } => {
                    image_inputs.remove(&var);

                    render(
                        start.elapsed().as_secs_f32(),
                        &mut width,
                        &mut height,
                        &mut staging_buffer,
                        &mut runner_th.write(),
                        &image_inputs,
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
                        "loading image {path:?} with dimensions width : {width} height: {height}"
                    );

                    let image_buffer = image.into_raw();

                    image_inputs.insert(
                        var,
                        crate::ImageDesc {
                            data: image_buffer,
                            width: im_width,
                            height: im_height,
                        },
                    );

                    render(
                        start.elapsed().as_secs_f32(),
                        &mut width,
                        &mut height,
                        &mut staging_buffer,
                        &mut runner_th.write(),
                        &image_inputs,
                        target.clone(),
                        status_th.clone(),
                    );
                }
                crate::AppMessage::LoadScript { path } => {
                    log::info!("loading script {path:?}");
                    let contents = match std::fs::read_to_string(&path) {
                        Ok(c) => c,
                        Err(e) => {
                            log::error!("{e:?}");
                            continue;
                        }
                    };

                    if let Some(old) = current_path {
                        watcher.unwatch(&old).unwrap();
                    }

                    watcher.watch(&path, RecursiveMode::NonRecursive).unwrap();

                    current_path = Some(path.clone());

                    if let Err(e) = runner_th.write().clear_script_parent_directory() {
                        log::error!("{e:?}");
                    }

                    if let Some(parent) = path.parent() {
                        runner_th
                            .write()
                            .set_script_parent_directory(parent.to_owned());
                    }

                    let out = runner_th
                        .write()
                        .load_script(contents, path.to_str().map(|s| s.to_owned()));

                    log_run(&out);

                    if out.is_err() {
                        *status_th.write() = RunnerStatus::InitFailed;
                    } else {
                        *status_th.write() = RunnerStatus::Normal { height, width };
                    }

                    render(
                        start.elapsed().as_secs_f32(),
                        &mut width,
                        &mut height,
                        &mut staging_buffer,
                        &mut runner_th.write(),
                        &image_inputs,
                        target.clone(),
                        status_th.clone(),
                    );
                }
                crate::AppMessage::ResizeOutput {
                    width: new_w,
                    height: new_h,
                } => {
                    staging_buffer = vec![255; (new_w * new_h * 4) as usize];
                    width = new_w as usize;
                    height = new_h as usize;
                    *status_th.write() = RunnerStatus::Normal { width, height };
                }
                crate::AppMessage::ReloadScript => {
                    if let Some(path) = current_path.as_ref() {
                        let contents = std::fs::read_to_string(path).unwrap();
                        let out = runner_th.write().load_script(contents, None);

                        log_run(&out);

                        if out.is_err() {
                            *status_th.write() = RunnerStatus::InitFailed;
                        } else {
                            *status_th.write() = RunnerStatus::Normal { width, height };
                            render(
                                start.elapsed().as_secs_f32(),
                                &mut width,
                                &mut height,
                                &mut staging_buffer,
                                &mut runner_th.write(),
                                &image_inputs,
                                target.clone(),
                                status_th.clone(),
                            );
                        }
                    }
                }
                crate::AppMessage::Render => render(
                    start.elapsed().as_secs_f32(),
                    &mut width,
                    &mut height,
                    &mut staging_buffer,
                    &mut runner_th.write(),
                    &image_inputs,
                    target.clone(),
                    status_th.clone(),
                ),
            }
        }
    });

    RunnerState {
        status,
        sender,
        runner,
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