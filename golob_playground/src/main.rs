mod inputs_panel;
mod util;

use eframe::*;
use egui::{TextureHandle, TextureOptions};
use golob_lib::PythonRunner;
use log::{error, info};
use notify::*;
use std::collections::HashMap;
use std::path::PathBuf;
use util::*;

#[derive(Debug)]
pub enum AppMessage {
    LoadImage { var: String, path: PathBuf },
    LoadScript { path: PathBuf },
    ReloadScript,
}

pub struct ImageDesc {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

pub struct AppState {
    pub texture: TextureHandle,
    pub start: std::time::Instant,
    pub rx: std::sync::mpsc::Receiver<AppMessage>,
    pub tx: std::sync::mpsc::Sender<AppMessage>,
    pub inputs: std::collections::HashMap<String, ImageDesc>,
    pub output: ImageDesc,
    pub current_file: Option<String>,
    pub staging_size: [usize; 2],
    pub script_path: Option<PathBuf>,
    pub input_panel_hidden: bool,
    pub show_resize_dialog: bool,
    pub needs_redraw: bool,
    pub draw_continuously: bool,
    pub eager_updates: bool,
    pub show_logs: bool,
}

#[derive(Debug)]
pub enum RunnerStatus {
    InitFailed,
    RunFailed,
    Normal,
}

pub struct PlayGround {
    runner: PythonRunner,
    status: RunnerStatus,
    watcher: notify::RecommendedWatcher,
    state: AppState,
}

impl PlayGround {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();

        if let Some(path) = path {
            let _ = tx.send(AppMessage::LoadScript { path });
        }

        let watcher_clone = tx.clone();
        let ctx = cc.egui_ctx.clone();

        let watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() {
                        let _ = watcher_clone.send(AppMessage::ReloadScript);
                        ctx.request_repaint();
                    }
                }
            },
            notify::Config::default(),
        )
        .unwrap();

        let (width, height) = (255, 255);
        let data = vec![255; width * height * 4];
        let output = ImageDesc {
            data,
            width: width as u32,
            height: height as u32,
        };

        let texture = cc.egui_ctx.load_texture(
            "output",
            egui::ColorImage::from_rgba_unmultiplied(
                [output.width as usize, output.height as usize],
                &output.data,
            ),
            TextureOptions::default(),
        );

        Self {
            runner: PythonRunner::default(),
            watcher,
            status: RunnerStatus::Normal,
            state: AppState {
                start: std::time::Instant::now(),
                texture,
                staging_size: [255, 255],
                current_file: None,
                output,
                rx,
                tx,
                inputs: HashMap::new(),
                script_path: None,
                input_panel_hidden: false,
                show_resize_dialog: false,
                draw_continuously: false,
                show_logs: false,
                needs_redraw: true,
                eager_updates: true,
            },
        }
    }
}

fn log_run(res: &std::result::Result<Option<String>, golob_lib::GolobulError>) {
    match res {
        Ok(Some(out)) => {
            info!("{}", out.trim_end());
        }
        Err(e) => {
            if let golob_lib::GolobulError::RuntimeError { stderr, stdout } = e {
                if let Some(stdout) = stdout {
                    info!("{}", stdout.trim_end());
                }
                error!("{stderr:?}");
            } else {
                error!("{e:?}");
            }
        }
        _ => {}
    };
}

impl eframe::App for PlayGround {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(message) = self.state.rx.try_recv() {
            match message {
                AppMessage::LoadScript { path } => {
                    info!("loading script {path:?}");
                    self.state.needs_redraw = true;
                    let contents = match std::fs::read_to_string(&path) {
                        Ok(c) => c,
                        Err(e) => {
                            error!("{e:?}");
                            continue;
                        }
                    };

                    if let Some(old) = &self.state.script_path {
                        self.watcher.unwatch(old).unwrap();
                    }

                    self.watcher
                        .watch(&path, RecursiveMode::NonRecursive)
                        .unwrap();

                    self.state.script_path = Some(path.clone());

                    if let Err(e) = self.runner.clear_script_parent_directory() {
                        error!("{e:?}");
                    }

                    if let Some(parent) = path.parent() {
                        self.runner.set_script_parent_directory(parent.to_owned());
                    }

                    let out = self
                        .runner
                        .load_script(contents, path.to_str().map(|s| s.to_owned()));

                    log_run(&out);

                    if out.is_err() {
                        self.state.show_logs = true;
                    } else {
                        self.state.current_file =
                            path.file_name().map(|s| s.to_str().unwrap().to_owned());
                    }
                }

                AppMessage::LoadImage { var, path } => {
                    self.state.needs_redraw = true;
                    let Ok(image) = image::open(&path) else {
                        continue;
                    };

                    let image = image.to_rgba8();
                    let [width, height] = [image.width(), image.height()];
                    info!(
                        "loading image {path:?} with dimensions width : {width} height: {height}"
                    );
                    let image_buffer = image.into_raw();
                    self.state.inputs.insert(
                        var,
                        ImageDesc {
                            data: image_buffer,
                            width,
                            height,
                        },
                    );
                    self.status = RunnerStatus::Normal;
                }

                AppMessage::ReloadScript => {
                    self.state.needs_redraw = true;
                    if let Some(path) = self.state.script_path.as_ref() {
                        let contents = std::fs::read_to_string(path).unwrap();
                        let out = self
                            .runner
                            .load_script(contents, path.to_str().map(|s| s.to_owned()));
                        log_run(&out);
                        if out.is_err() {
                            self.status = RunnerStatus::InitFailed;
                            self.state.show_logs = true;
                        } else {
                            self.status = RunnerStatus::Normal;
                        }
                    }
                }
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.state.input_panel_hidden = !self.state.input_panel_hidden;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let lb = util::compute_letterbox(
                [
                    self.state.output.width as usize,
                    self.state.output.height as usize,
                ],
                ctx.screen_rect(),
            );
            egui::Image::new(&self.state.texture).paint_at(ui, lb);
        });

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }

                    if ui.button("Load Python Script").clicked() {
                        launch_script_dialog(self.state.tx.clone(), ctx.clone());
                    }
                });

                ui.menu_button("Options", |ui| {
                    if ui.button("Resize Output Image").clicked() {
                        self.state.show_resize_dialog = true;
                    }
                    ui.checkbox(&mut self.state.draw_continuously, "animated");
                    ui.checkbox(&mut self.state.eager_updates, "eagerly update inputs");
                    ui.checkbox(&mut self.state.show_logs, "logs");
                });
            });
        });

        if self.state.show_resize_dialog {
            util::resize_dialog(&mut self.state, &mut self.status, ctx);
        }

        egui::SidePanel::new(egui::panel::Side::Left, "User Inputs").show_animated(
            ctx,
            !self.state.input_panel_hidden,
            |ui| {
                ui.vertical_centered_justified(|ui| {
                    match self.state.current_file {
                        None => ui.label("No File Loaded"),
                        Some(ref s) => ui.label(format!("Loaded File: {s}")),
                    };
                });

                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Label and close button.
                    ui.vertical_centered_justified(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("User Inputs").size(15.0));
                            if ui.button("<< [Esc]").clicked() {
                                self.state.input_panel_hidden = true;
                            }
                        });

                        ui.separator();
                    });

                    let mut changed = false;
                    for (name, ref mut val) in self.runner.iter_inputs_mut() {
                        changed |= inputs_panel::input_widget(ctx, ui, &mut self.state, name, val);
                    }

                    if self.state.eager_updates {
                        self.state.needs_redraw |= changed;
                    } else {
                        ui.vertical_centered(|ui| {
                            let clicked = ui.button("Redraw").clicked();
                            self.state.needs_redraw |= clicked;
                            if clicked {
                                self.status = RunnerStatus::Normal;
                            }
                        });
                    }
                });
            },
        );

        // run the actual render code
        // this only loops because I force a second render on
        // output image reallocation. it looks less weird.
        // we blit to the center of any texture that resizes the output
        // which looks fine in AE but bad here.
        while matches!(self.status, RunnerStatus::Normal) && self.state.needs_redraw {
            let start = std::time::Instant::now();

            self.runner
                .set_time(self.state.start.elapsed().as_secs_f32());

            self.state.output.data.fill(0);
            let o = golob_lib::OutDesc {
                fmt: golob_lib::ImageFormat::Rgba8,
                data: &mut self.state.output.data,
                height: self.state.output.height,
                width: self.state.output.width,
                stride: None,
            };

            let mut pass = self.runner.create_render_pass(o);

            for (name, image) in self.state.inputs.iter() {
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
                self.status = RunnerStatus::RunFailed;
                self.state.show_logs = true;
            }

            log_run(&out);

            if self.runner.requested_output_resize().is_some_and(|size| {
                size.width != self.state.output.width || size.height != self.state.output.height
            }) {
                let size = self.runner.requested_output_resize().unwrap();
                self.state.output.data = vec![0; (size.width * size.height * 4) as usize];
                self.state.output.width = size.width;
                self.state.output.height = size.height;
            } else {
                let data = egui::ColorImage::from_rgba_unmultiplied(
                    [
                        self.state.output.width as usize,
                        self.state.output.height as usize,
                    ],
                    &self.state.output.data,
                );
                self.state.texture.set(data, Default::default());
                self.state.needs_redraw = false;
                let dur = start.elapsed().as_secs_f32();
                info!("render took: {dur} secs");
            }
        }

        egui::Window::new("Logs")
            .open(&mut self.state.show_logs)
            .show(ctx, |ui| {
                egui_logger::logger_ui(ui);
            });

        if self.state.draw_continuously {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            self.state.needs_redraw = true;
        }
    }
}

fn main() -> eframe::Result<()> {
    egui_logger::init().unwrap();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 800.0])
            .with_min_inner_size([300.0, 220.0]),
        ..Default::default()
    };

    let mut script_path = None;

    let args_owned: Vec<String> = std::env::args().collect();
    let args: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();

    match args.as_slice() {
        [.., "--file", file_path] => {
            let path = PathBuf::from(file_path);
            if path.exists() {
                script_path = Some(path);
            } else {
                eprintln!("File does not exist: {:?}", path);
                return Ok(());
            }
        }
        [_, "--file"] => {
            eprintln!("Missing file path after --file flag");
            return Ok(());
        }
        _ => {}
    };

    eframe::run_native(
        "Golobulus Playground",
        native_options,
        Box::new(|cc| Box::new(PlayGround::new(cc, script_path))),
    )
}
