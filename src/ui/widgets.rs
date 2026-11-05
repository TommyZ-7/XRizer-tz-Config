//! Shared widgets.

use eframe::egui;

use crate::app::ToastKind;

pub fn section_title(ui: &mut egui::Ui, title: &str, help: &str) {
    ui.add_space(4.0);
    ui.heading(title);
    help_label(ui, help);
    ui.add_space(4.0);
}

pub fn status_badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.label(egui::RichText::new(text).color(color).strong());
}

pub fn warning_banner(ui: &mut egui::Ui, text: &str) {
    let color = egui::Color32::from_rgb(230, 180, 70);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("⚠").color(color).strong());
        ui.label(egui::RichText::new(text).color(color));
    });
}

pub fn error_banner(ui: &mut egui::Ui, text: &str) {
    let color = egui::Color32::from_rgb(240, 110, 110);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("✖").color(color).strong());
        ui.label(egui::RichText::new(text).color(color));
    });
}

pub fn help_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(ui.visuals().weak_text_color()),
    );
}

pub fn mono_path(ui: &mut egui::Ui, path: &std::path::Path) {
    ui.label(egui::RichText::new(path.display().to_string()).monospace());
}

pub fn toast(ui: &mut egui::Ui, text: &str, kind: ToastKind) {
    let color = match kind {
        ToastKind::Info => egui::Color32::from_rgb(150, 190, 240),
        ToastKind::Success => egui::Color32::from_rgb(130, 210, 140),
        ToastKind::Error => egui::Color32::from_rgb(240, 110, 110),
    };
    ui.label(egui::RichText::new(text).color(color).strong());
}
