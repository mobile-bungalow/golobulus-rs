use crate::*;
use std::sync::mpsc::Sender;

pub fn launch_script_dialog(
    sender: Sender<AppMessage>,
    ctx: egui::Context,
    current_script: Arc<RwLock<Option<String>>>,
) {
    std::thread::spawn(move || {
        let home_dir = match homedir::get_my_home() {
            Ok(Some(home)) => home,
            _ => "/".into(),
        };

        let home_dir = current_script
            .read()
            .unwrap()
            .as_ref()
            .map(|p| PathBuf::from(p).parent().unwrap().to_owned())
            .unwrap_or(home_dir);

        let Some(file_path) = rfd::FileDialog::new()
            .set_directory(home_dir)
            .add_filter("python", &["py"])
            .pick_file()
        else {
            return;
        };

        let path = file_path;

        *current_script.write().unwrap() =
            Some(path.file_name().unwrap().to_string_lossy().to_string());

        let _ = sender.send(AppMessage::LoadScript { path });

        ctx.request_repaint();
    });
}

pub fn launch_image_dialog(
    sender: Sender<AppMessage>,
    ctx: egui::Context,
    var: String,
    entries: Arc<RwLock<HashMap<String, PathBuf>>>,
) {
    std::thread::spawn(move || {
        let home_dir = match homedir::get_my_home() {
            Ok(Some(home)) => home,
            _ => "/".into(),
        };

        let first_image = entries
            .read()
            .ok()
            .and_then(|ok| ok.values().next().cloned());

        let home_dir = first_image.unwrap_or(home_dir);

        let Some(file_path) = rfd::FileDialog::new()
            .set_directory(home_dir)
            .add_filter("image", &["png", "jpeg", "jpg", "exr", "webm"])
            .pick_file()
        else {
            return;
        };

        let path = file_path;
        let path_clone = path.clone();
        entries.write().unwrap().insert(var.clone(), path_clone);
        let _ = sender.send(AppMessage::LoadImage { path, var });
        ctx.request_repaint();
    });
}

pub fn compute_letterbox(texture_size: [usize; 2], screen_rect: egui::Rect) -> egui::Rect {
    // pad letterbox
    let mut new_rect = screen_rect;
    new_rect.min += egui::vec2(0.0, 20.0);
    let screen_rect = new_rect;

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
