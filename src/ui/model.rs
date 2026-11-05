//! Model tab.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use crate::app::{App, ConfirmAction, ConfirmDialog};
use crate::core::config::{self, Preset};
use crate::core::model::{self, ModelInfo};
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_title(
        ui,
        "モデル",
        "XRU1 モデルを検証し、既定の upscaler_model.xru へ安全に配置します。",
    );

    if let Some(error) = &app.model.error {
        widgets::error_banner(ui, error);
    }
    if let Some(message) = &app.model.message {
        ui.label(egui::RichText::new(message).color(egui::Color32::from_rgb(130, 210, 140)));
    }
    if app.model.busy.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("配置中...");
        });
    }

    let normalized =
        config::normalize_model_path(Some(&app.config.model_path_text), app.env.home.as_deref());
    let effective_path = match &normalized {
        Ok(Some(path)) => path.clone(),
        Ok(None) => app.model.target.clone(),
        Err(_) => app.model.target.clone(),
    };

    ui.horizontal(|ui| {
        ui.label("使用モデル:");
        widgets::mono_path(ui, &effective_path);
    });
    if let Err(error) = &normalized {
        widgets::error_banner(ui, error);
    }
    if let Ok(Some(path)) = &normalized
        && path != &app.model.target
    {
        widgets::warning_banner(
            ui,
            "model_path が既定パス以外を指しています。既定パスへの配置は使われません。",
        );
        if ui.button("model_path を null に戻す").clicked() {
            app.config.model_path_text.clear();
            app.config.recompute_dirty();
        }
    }

    let current = if effective_path.exists() {
        Some(model::inspect(&effective_path))
    } else {
        None
    };
    match &current {
        Some(Ok(info)) => {
            show_model_info(ui, "現在", info);
            show_modified(ui, &effective_path);
        }
        Some(Err(error)) => {
            widgets::error_banner(ui, &format!("使用モデルを読み込めません: {error}"));
        }
        None => {
            widgets::warning_banner(
                ui,
                "使用モデルがありません。ml は FSR にフォールバックします。",
            );
        }
    }

    ui.add_space(8.0);
    ui.label(egui::RichText::new("既定パスへの配置").strong());
    ui.horizontal(|ui| {
        ui.label("配置先:");
        widgets::mono_path(ui, &app.model.target);
    });

    let mut source_changed = false;
    ui.horizontal(|ui| {
        ui.label("ソース .xru");
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.model.source_text)
                .desired_width(420.0)
                .hint_text("/path/to/model.xru"),
        );
        if response.changed() {
            source_changed = true;
        }
        if ui.button("参照...").clicked() {
            let mut dialog = rfd::FileDialog::new().add_filter("XRU model", &["xru"]);
            if let Some(dir) = &app.settings.last_model_dir {
                dialog = dialog.set_directory(dir);
            }
            if let Some(path) = dialog.pick_file() {
                app.model.source_text = path.display().to_string();
                if let Some(dir) = path.parent() {
                    app.settings.last_model_dir = Some(dir.display().to_string());
                    app.settings_dirty = true;
                }
                source_changed = true;
            }
        }
    });
    if source_changed {
        app.model.refresh_source();
    }
    match &app.model.source {
        Some(Ok(info)) => {
            show_model_info(ui, "ソース", info);
            if let Some(path) = source_path(&app.model.source_text) {
                show_modified(ui, &path);
            }
        }
        Some(Err(error)) => {
            widgets::error_banner(ui, &format!("ソースを読み込めません: {error}"));
        }
        None => widgets::help_label(ui, "XRU1 モデルを選択してください。"),
    }

    let source = source_path(&app.model.source_text);
    let source_is_target = source
        .as_ref()
        .is_some_and(|path| path == &app.model.target);
    let can_install = app.model.busy.is_none()
        && app.model.source.as_ref().is_some_and(Result::is_ok)
        && source.is_some()
        && !source_is_target;
    let mut install_clicked = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_install, egui::Button::new("既定パスへ配置"))
            .clicked()
        {
            install_clicked = true;
        }
        if source_is_target {
            widgets::help_label(ui, "既に既定パスにあります。");
        }
    });

    if install_clicked && let Some(source) = source {
        if app.model.target.exists() {
            app.confirm = Some(ConfirmDialog {
                title: "モデルを置き換える".to_string(),
                body: format!(
                    "既存の {} を .bak に退避して置き換えます。",
                    app.model.target.display()
                ),
                confirm_label: "置き換える".to_string(),
                action: ConfirmAction::InstallModel {
                    source,
                    target: app.model.target.clone(),
                },
            });
        } else {
            app.start_model_install(source, app.model.target.clone());
        }
    }

    if app.config.form.preset != Preset::Ml {
        widgets::help_label(ui, "プリセットが ml でないため、配置しても使われません。");
    }
}

fn source_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn show_model_info(ui: &mut egui::Ui, label: &str, info: &ModelInfo) {
    egui::Grid::new(("model_info", label))
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("種別");
            ui.label(info.kind.label());
            ui.end_row();
            ui.label("feature_count");
            ui.label(info.feature_count.to_string());
            ui.end_row();
            ui.label("hidden / channels");
            ui.label(format!("{} / {}", info.hidden, info.channels));
            ui.end_row();
            ui.label("レイヤー");
            ui.label(
                info.layer_shapes
                    .iter()
                    .map(|(input, output)| format!("{input}→{output}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            ui.end_row();
        });
}

fn show_modified(ui: &mut egui::Ui, path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        let size = size_label(metadata.len());
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.elapsed().ok())
            .map(relative_time)
            .unwrap_or_else(|| "不明".to_string());
        widgets::help_label(ui, &format!("サイズ: {size} / 更新: {modified}"));
    }
}

fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn relative_time(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds} 秒前")
    } else if seconds < 3600 {
        format!("{} 分前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 時間前", seconds / 3600)
    } else {
        format!("{} 日前", seconds / 86_400)
    }
}
