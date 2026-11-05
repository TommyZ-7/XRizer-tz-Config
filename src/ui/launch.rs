//! Launch options tab (implemented in Task 14).

use eframe::egui;

pub fn show(_app: &mut crate::app::App, ui: &mut egui::Ui) {
    ui.heading("起動オプション");
    ui.label("準備中");
}
