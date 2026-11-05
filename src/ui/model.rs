//! Model tab (implemented in Task 12).

use eframe::egui;

pub fn show(_app: &mut crate::app::App, ui: &mut egui::Ui) {
    ui.heading("モデル");
    ui.label("準備中");
}
