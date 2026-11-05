//! Information tab (implemented in Task 15).

use eframe::egui;

pub fn show(_app: &mut crate::app::App, ui: &mut egui::Ui) {
    ui.heading("情報");
    ui.label("準備中");
}
