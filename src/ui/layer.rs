//! Vulkan layer tab (implemented in Task 13).

use eframe::egui;

pub fn show(_app: &mut crate::app::App, ui: &mut egui::Ui) {
    ui.heading("Vulkan レイヤー");
    ui.label("準備中");
}
