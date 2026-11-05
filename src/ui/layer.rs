//! Vulkan layer tab.

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use crate::app::{App, ConfirmAction, ConfirmDialog};
use crate::core::layer::{self, LayerStatus, SoInfo};
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_title(
        ui,
        "Vulkan レイヤー",
        "ml の DP4A 経路にだけ必要です。fast / balanced / fsr には不要で、失敗時は本体が FSR にフォールバックします。",
    );

    if let Some(error) = &app.layer.error {
        widgets::error_banner(ui, error);
    }
    if app.layer.busy.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("実行中...");
        });
    }

    ui.label(egui::RichText::new("レイヤー .so").strong());
    let mut so_changed = false;
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut app.layer.so_text)
                .desired_width(460.0)
                .hint_text(".../libxrizer_upscaler_layer.so"),
        );
        if response.changed() {
            so_changed = true;
        }
        if ui.button("参照...").clicked() {
            let mut dialog = rfd::FileDialog::new().add_filter("Vulkan layer", &["so"]);
            if !app.layer.so_text.trim().is_empty()
                && let Some(parent) = Path::new(app.layer.so_text.trim()).parent()
            {
                dialog = dialog.set_directory(parent);
            }
            if let Some(path) = dialog.pick_file() {
                app.layer.so_text = path.display().to_string();
                app.settings.layer_so_path = Some(app.layer.so_text.clone());
                app.settings_dirty = true;
                so_changed = true;
            }
        }
        if ui.button("自動検出").clicked()
            && let Some(path) = layer::auto_detect_so(&app.env)
        {
            app.layer.so_text = path.display().to_string();
            app.settings.layer_so_path = Some(app.layer.so_text.clone());
            app.settings_dirty = true;
            so_changed = true;
        }
    });
    if so_changed {
        app.layer.refresh_so();
        app.settings.layer_so_path = Some(app.layer.so_text.clone());
        app.settings_dirty = true;
    }
    match &app.layer.so {
        Some(Ok(info)) => show_so_info(ui, info),
        Some(Err(error)) => {
            widgets::error_banner(ui, error);
        }
        None => widgets::help_label(
            ui,
            "ビルド済みの libxrizer_upscaler_layer.so を選択してください。",
        ),
    }

    ui.add_space(8.0);
    ui.label(egui::RichText::new("導入状態").strong());
    let snapshot = app.layer.snapshot.clone();
    match &snapshot.status {
        LayerStatus::Installed {
            library_path,
            lib_size,
        } => {
            widgets::status_badge(ui, "導入済み", egui::Color32::from_rgb(130, 210, 140));
            if let Some(paths) = &snapshot.paths {
                ui.horizontal(|ui| {
                    ui.label("manifest:");
                    widgets::mono_path(ui, &paths.manifest);
                });
            }
            ui.horizontal(|ui| {
                ui.label("ライブラリ:");
                widgets::mono_path(ui, library_path);
            });
            ui.label(format!("サイズ: {}", size_label(*lib_size)));
            if let Some(hash) = app.layer.installed_hash {
                widgets::help_label(ui, &format!("導入済みハッシュ: {hash:016x}"));
            }
        }
        LayerStatus::Partial { detail } => {
            widgets::status_badge(ui, "不完全", egui::Color32::from_rgb(230, 180, 70));
            widgets::warning_banner(ui, detail);
            if let Some(paths) = &snapshot.paths {
                ui.horizontal(|ui| {
                    ui.label("manifest:");
                    widgets::mono_path(ui, &paths.manifest);
                });
                ui.horizontal(|ui| {
                    ui.label("ライブラリ:");
                    widgets::mono_path(ui, &paths.lib);
                });
            }
        }
        LayerStatus::NotInstalled => {
            widgets::status_badge(ui, "未導入", egui::Color32::GRAY);
            if let Some(paths) = &snapshot.paths {
                ui.horizontal(|ui| {
                    ui.label("manifest:");
                    widgets::mono_path(ui, &paths.manifest);
                });
            }
        }
        LayerStatus::Error { detail } => {
            widgets::status_badge(ui, "エラー", egui::Color32::from_rgb(240, 110, 110));
            widgets::error_banner(ui, detail);
        }
    }

    if let (Some(Ok(selected)), Some(installed)) = (&app.layer.so, app.layer.installed_hash) {
        if selected.hash == installed {
            ui.label(
                egui::RichText::new("選択中の .so と導入済みは一致しています")
                    .color(egui::Color32::from_rgb(130, 210, 140)),
            );
        } else {
            widgets::warning_banner(
                ui,
                "選択中の .so と導入済みが異なります。再インストールしてください。",
            );
        }
    }

    let can_install = app.layer.busy.is_none() && app.layer.so.as_ref().is_some_and(Result::is_ok);
    let can_remove =
        app.layer.busy.is_none() && !matches!(&snapshot.status, LayerStatus::NotInstalled);
    let mut install_clicked = false;
    let mut remove_clicked = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(can_install, egui::Button::new("インストール / 更新"))
            .clicked()
        {
            install_clicked = true;
        }
        if ui
            .add_enabled(can_remove, egui::Button::new("削除"))
            .clicked()
        {
            remove_clicked = true;
        }
        if ui.button("状態を再読込").clicked() {
            app.layer.refresh(&app.env);
            app.layer.refresh_so();
        }
    });

    if install_clicked {
        app.start_layer_install(PathBuf::from(app.layer.so_text.trim()));
    }
    if remove_clicked {
        app.confirm = Some(ConfirmDialog {
            title: "Vulkan レイヤーを削除".to_string(),
            body: "ユーザーディレクトリに導入した manifest とライブラリを削除します。".to_string(),
            confirm_label: "削除".to_string(),
            action: ConfirmAction::RemoveLayer,
        });
    }

    if !app.layer.log.is_empty() {
        egui::CollapsingHeader::new("インストーラのログ").show(ui, |ui| {
            let mut log = app.layer.log.clone();
            ui.add(
                egui::TextEdit::multiline(&mut log)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        });
    }

    ui.add_space(8.0);
    widgets::help_label(
        ui,
        "導入状態はファイルの存在で判定します。実際に Vulkan ローダーが読んだかは VK_LOADER_DEBUG=layer で確認してください。",
    );
    widgets::help_label(
        ui,
        "レイヤーはゲーム起動時に読まれます。導入・削除後はゲームを再起動してください。",
    );
    widgets::help_label(
        ui,
        "起動オプションタブで XRIZER_UPSCALER_VK_LAYER=1 を有効にする必要があります。",
    );
    widgets::help_label(ui, "同梱元: XRizer main e6fbdd4（GPL-3.0-or-later）。");
}

fn show_so_info(ui: &mut egui::Ui, info: &SoInfo) {
    ui.label(format!(
        "サイズ: {} / ハッシュ: {:016x}",
        size_label(info.size),
        info.hash
    ));
    if let Some(modified) = info.modified
        && let Ok(elapsed) = modified.elapsed()
    {
        widgets::help_label(ui, &format!("更新: {}", relative_time(elapsed)));
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
