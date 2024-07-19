mod background_thread;
mod inputs_panel;
mod util;

use eframe::*;
use egui::TextureHandle;
use std::sync::{Arc, RwLock};
use std::{collections::HashMap, path::PathBuf};
use util::*;

#[derive(Debug)]
pub enum AppMessage {
    LoadImage {
        var: String,
        path: PathBuf,
    },
    UnloadImage {
        var: String,
    },
    ChangeFilterMode {
        mode: egui::TextureFilter,
    },
    LoadScript {
        path: PathBuf,
    },
    LoadVenv {
        path: PathBuf,
    },
    ResizeOutput {
        width: u32,
        height: u32,
    },
    ScreenShot {
        params: Option<(u32, u32, egui::TextureFilter)>,
    },
    ReloadScript,
    Render,
}

pub struct ImageDesc {
    data: Vec<u8>,
    width: u32,
    height: u32,
}

pub struct AppState {
    pub texture: TextureHandle,
    pub loaded_images: Arc<RwLock<std::collections::HashMap<String, PathBuf>>>,
    pub last_render: std::time::Instant,
    pub last_render_dim: [usize; 2],
    pub current_file: Arc<RwLock<Option<String>>>,
    pub input_panel_hidden: bool,
    pub draw_continuously: bool,
    pub eager_updates: bool,
    pub show_logs: bool,
    pub filter_type: egui::TextureFilter,
}

pub struct PlayGround {
    runner: background_thread::RunnerState,
    state: AppState,
}

impl PlayGround {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let (width, height) = (255, 255);
        let data = vec![0; width * height * 4];

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
            egui::TextureOptions::default(),
        );

        let runner = background_thread::spawn_render_thread(texture.clone());

        if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
            let mut site_packages = PathBuf::from(venv);
            site_packages.push("lib");
            // FIXME: use the python version linked from the build env
            site_packages.push("python3.12");
            site_packages.push("site-packages");

            let _ = runner.sender.send(AppMessage::LoadVenv {
                path: site_packages,
            });
        }

        if let Some(path) = path.clone() {
            let _ = runner.sender.send(AppMessage::LoadScript { path });
        }

        Self {
            runner,
            state: AppState {
                texture,
                last_render: std::time::Instant::now(),
                last_render_dim: [255, 255],
                loaded_images: Arc::default(),
                current_file: Arc::new(RwLock::new(
                    path.and_then(|p| p.to_str().map(|s| s.to_owned())),
                )),
                input_panel_hidden: false,
                draw_continuously: false,
                show_logs: false,
                eager_updates: true,
                filter_type: egui::TextureFilter::Linear,
            },
        }
    }
}

impl eframe::App for PlayGround {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.state.input_panel_hidden = !self.state.input_panel_hidden;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let stat_copy = (*self.runner.status.read()).clone();
            match stat_copy {
                background_thread::RunnerStatus::InitFailed => {
                    self.state.show_logs = true;
                    *self.state.current_file.write().unwrap() = None;
                    *self.runner.status.write() = background_thread::RunnerStatus::Busy;
                }
                background_thread::RunnerStatus::RunFailed => {
                    self.state.show_logs = true;
                    *self.runner.status.write() = background_thread::RunnerStatus::Busy;
                }
                background_thread::RunnerStatus::Busy => {
                    if self.state.last_render.elapsed() < std::time::Duration::from_millis(16 * 10)
                    {
                        let lb =
                            util::compute_letterbox(self.state.last_render_dim, ctx.screen_rect());
                        egui::Image::new(&self.state.texture).paint_at(ui, lb);
                    } else {
                        let space = ui.available_rect_before_wrap();
                        let mut rect = ui.available_rect_before_wrap();
                        rect.min.x += space.width() / 6.0;
                        rect.min.y += space.height() / 6.0;
                        rect.max.x -= space.width() / 4.0;
                        rect.max.y -= space.height() / 4.0;
                        egui::widgets::Spinner::new().paint_at(ui, rect);
                    }
                }
                background_thread::RunnerStatus::Normal { width, height } => {
                    let lb = util::compute_letterbox([width, height], ctx.screen_rect());
                    self.state.last_render_dim = [width, height];
                    self.state.last_render = std::time::Instant::now();
                    egui::Image::new(&self.state.texture).paint_at(ui, lb);
                }
            };
        });

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }

                    if ui.button("Load Python Script").clicked() {
                        launch_script_dialog(
                            self.runner.sender.clone(),
                            ctx.clone(),
                            self.state.current_file.clone(),
                        );
                    }
                });

                ui.menu_button("Tools", |ui| {
                    if ui.button("Take Screenshot").clicked() {
                        self.runner
                            .sender
                            .send(AppMessage::ScreenShot { params: None })
                            .unwrap();
                    }

                    if ui.button("Take Screenshot at Window Resolution").clicked() {
                        let lb =
                            util::compute_letterbox(self.state.last_render_dim, ctx.screen_rect());
                        self.runner
                            .sender
                            .send(AppMessage::ScreenShot {
                                params: Some((
                                    lb.width() as u32,
                                    lb.height() as u32,
                                    self.state.filter_type,
                                )),
                            })
                            .unwrap();
                    }
                });

                ui.menu_button("Options", |ui| {
                    let before = self.state.filter_type;
                    egui::ComboBox::from_label("Select Filter Type")
                        .selected_text(format!("{:?}", self.state.filter_type))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.state.filter_type,
                                egui::TextureFilter::Nearest,
                                "Nearest",
                            );
                            ui.selectable_value(
                                &mut self.state.filter_type,
                                egui::TextureFilter::Linear,
                                "Linear",
                            );
                        });
                    if before != self.state.filter_type {
                        self.runner
                            .sender
                            .send(AppMessage::ChangeFilterMode {
                                mode: self.state.filter_type,
                            })
                            .unwrap();
                    }

                    ui.separator();

                    ui.checkbox(&mut self.state.draw_continuously, "animate script");
                    ui.checkbox(&mut self.state.eager_updates, "eagerly update inputs");
                    ui.checkbox(&mut self.state.show_logs, "show logs");
                });
            });
        });

        egui::SidePanel::new(egui::panel::Side::Left, "User Inputs").show_animated(
            ctx,
            !self.state.input_panel_hidden,
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Label and close button.

                    ui.vertical_centered_justified(|ui| {
                        match self.state.current_file.read().unwrap().as_ref() {
                            None => ui.label("No File Loaded"),
                            Some(ref s) => ui.label(format!("Watched File: {s}")),
                        };
                    });

                    ui.separator();

                    let mut changed = false;
                    let sender = &self.runner.sender;
                    let mut runner = self.runner.runner.write();
                    for (name, ref mut val) in runner.iter_inputs_mut() {
                        changed |=
                            inputs_panel::input_widget(ctx, ui, &mut self.state, sender, name, val);
                    }

                    if self.state.eager_updates && changed {
                        self.runner.sender.send(AppMessage::Render).unwrap();
                    }

                    if !self.state.eager_updates {
                        ui.vertical_centered(|ui| {
                            if ui.button("Redraw").clicked() {
                                self.runner.sender.send(AppMessage::Render).unwrap();
                            }
                        });
                    }

                    ui.add_space(ui.available_height() - ui.spacing().interact_size.y - 15.0);

                    ui.separator();

                    ui.centered_and_justified(|ui| {
                        if ui.button("<< Close [Esc]").clicked() {
                            self.state.input_panel_hidden = true;
                        }
                    });
                });
            },
        );

        egui::Window::new("Logs")
            .open(&mut self.state.show_logs)
            .show(ctx, |ui| {
                egui_logger::logger_ui(ui);
            });

        if self.state.draw_continuously {
            self.runner.sender.send(AppMessage::Render).unwrap();
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
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
