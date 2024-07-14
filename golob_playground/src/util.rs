use crate::*;
use std::sync::mpsc::Sender;

pub fn launch_script_dialog(sender: Sender<AppMessage>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let file_path = tinyfiledialogs::open_file_dialog("Load a python script", "/", None);

        if let Some(file_path) = file_path {
            let _ = sender.send(AppMessage::LoadScript {
                path: std::path::PathBuf::from(file_path),
            });
            ctx.request_repaint();
        }
    });
}

pub fn launch_image_dialog(sender: Sender<AppMessage>, ctx: egui::Context, var: String) {
    std::thread::spawn(move || {
        let file_path = tinyfiledialogs::open_file_dialog("Load an Image or Video", "/", None);

        if let Some(file_path) = file_path {
            let _ = sender.send(AppMessage::LoadImage {
                path: std::path::PathBuf::from(file_path),
                var,
            });
            ctx.request_repaint();
        }
    });
}

pub fn resize_dialog(state: &mut AppState, status: &mut RunnerStatus, ctx: &egui::Context) {
    egui::Window::new("Resize Output Image").show(ctx, |ui| {
        ui.add(egui::Slider::new(&mut state.staging_size[0], 1..=4096).text("Width"));
        ui.add(egui::Slider::new(&mut state.staging_size[1], 1..=4096).text("Height"));

        if ui.button("Resize").clicked() {
            state.output.data = vec![255; state.staging_size[0] * state.staging_size[1] * 4];
            state.output.width = state.staging_size[0] as u32;
            state.output.height = state.staging_size[1] as u32;

            state.show_resize_dialog = false;
            state.needs_redraw = true;
            *status = RunnerStatus::Normal;
            info!("resizing output image");
        }

        if ui.button("Cancel").clicked() {
            state.show_resize_dialog = false;
        }
    });
}

pub fn compute_letterbox(texture_size: [usize; 2], screen_rect: egui::Rect) -> egui::Rect {
    let texture_aspect_ratio = texture_size[0] as f32 / texture_size[1] as f32;
    let screen_aspect_ratio = screen_rect.width() / screen_rect.height();

    if texture_aspect_ratio > screen_aspect_ratio {
        let height = screen_rect.width() / texture_aspect_ratio;
        let y_offset = (screen_rect.height() - height) / 2.0;
        egui::Rect::from_min_size(
            egui::Pos2::new(screen_rect.min.x, screen_rect.min.y + y_offset),
            egui::Vec2::new(screen_rect.width(), height),
        )
    } else {
        let width = screen_rect.height() * texture_aspect_ratio;
        let x_offset = (screen_rect.width() - width) / 2.0;
        egui::Rect::from_min_size(
            egui::Pos2::new(screen_rect.min.x + x_offset, screen_rect.min.y),
            egui::Vec2::new(width, screen_rect.height()),
        )
    }
}
