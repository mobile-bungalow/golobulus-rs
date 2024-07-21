use std::sync::mpsc::Sender;

use crate::{launch_image_dialog, AppMessage};

fn point_selector(ui: &mut egui::Ui, name: &str, input: &mut golob_lib::Cfg<[f32; 2]>) {
    ui.horizontal(|ui| {
        ui.label(name);
        ui.label("X");
        ui.add(
            egui::DragValue::new(&mut input.current[0]).clamp_range(input.min[0]..=input.max[0]),
        );
        ui.label("Y");
        ui.add(
            egui::DragValue::new(&mut input.current[1]).clamp_range(input.min[1]..=input.max[1]),
        );
    });

    egui_plot::Plot::new(name)
        .include_x(input.max[0])
        .include_x(input.min[0])
        .include_y(input.max[1])
        .include_y(input.min[1])
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_drag(false)
        .allow_double_click_reset(false)
        .view_aspect(2.0)
        .show(ui, |plot_ui| {
            if plot_ui.response().clicked()
                || plot_ui.pointer_coordinate_drag_delta() != egui::Vec2::ZERO
            {
                if let Some(p) = plot_ui.pointer_coordinate() {
                    input.current = [
                        p.x.clamp(input.min[0] as f64, input.max[0] as f64) as f32,
                        p.y.clamp(input.min[1] as f64, input.max[1] as f64) as f32,
                    ];
                }
            }
            let point = egui_plot::Points::new(egui_plot::PlotPoints::Owned(vec![
                egui_plot::PlotPoint::new(input.current[0], input.current[1]),
            ]))
            .radius(5.0);
            plot_ui.points(point);
        });
}

pub fn input_widget(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    state: &mut crate::AppState,
    message_queue: &Sender<AppMessage>,
    name: &str,
    val: &mut golob_lib::Variant,
) -> bool {
    let before = val.clone();
    match val {
        golob_lib::Variant::Image(d) => match d.current {
            golob_lib::Image::Input => {
                file_selector(ui, ctx, name, state, message_queue);
            }
            golob_lib::Image::Output => {}
        },
        golob_lib::Variant::Bool(b) => {
            if ui.radio(b.current, name).clicked() {
                b.current = !b.current;
            }
        }
        golob_lib::Variant::TaggedInt(ti) => {
            let current = ti
                .tags
                .iter()
                .find_map(|(str, val)| if ti.value == *val { Some(str) } else { None });

            egui::ComboBox::from_label(name)
                .selected_text(current.unwrap())
                .show_ui(ui, |ui| {
                    for opt in ti.tags.iter() {
                        ui.selectable_value(&mut ti.value, *opt.1, opt.0);
                    }
                });
        }
        golob_lib::Variant::Color(cfg) => {
            let _ = ui.horizontal(|ui| {
                ui.color_edit_button_rgba_unmultiplied(&mut cfg.current);
                ui.label(name);
            });
        }
        golob_lib::Variant::Int(v) => {
            ui.add(egui::Slider::new(&mut v.current, v.min..=v.max).text(name));
            ui.add_space(10.0);
        }
        golob_lib::Variant::Float(v) => {
            ui.add(egui::Slider::new(&mut v.current, v.min..=v.max).text(name));
            ui.add_space(10.0);
        }
        golob_lib::Variant::Vector2(ref mut v) => {
            point_selector(ui, name, v);
        }
    }
    before != *val
}

fn file_selector(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    name: &str,
    app_state: &mut crate::AppState,
    message_queue: &Sender<AppMessage>,
) {
    ui.horizontal(|ui| {
        ui.label(name);
        if app_state.loaded_images.read().unwrap().contains_key(name) {
            ui.label("Loaded");
            if ui.button("X").clicked() {
                message_queue
                    .send(AppMessage::UnloadImage {
                        var: name.to_owned(),
                    })
                    .unwrap();
                app_state.loaded_images.write().unwrap().remove(name);
            }
        } else if ui.button("Load Image").clicked() {
            launch_image_dialog(
                message_queue.clone(),
                ctx.clone(),
                name.to_owned(),
                app_state.loaded_images.clone(),
            );
        }
    });
}
