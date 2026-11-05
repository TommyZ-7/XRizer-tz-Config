//! Information tab.

use std::path::Path;

use eframe::egui;

use crate::app::{App, ConfigState};
use crate::core::config;
use crate::core::layer::LayerStatus;
use crate::core::scale::ScaleState;
use crate::core::settings;
use crate::ui::widgets;

const SCRIPT_SHA256: &str = "9b1d10894dd4d261b4b1f34092815a58be831836207893d54d02646215ba3f60";
const MANIFEST_SHA256: &str = "07755c72b67a92081c12795246153e6907efd13ae59e8e04d254aa7f2a1342a8";

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_title(
        ui,
        "情報",
        "アプリのバージョン、解決したパス、および GUI が参照している環境値を表示します。",
    );

    if let Some(warning) = &app.settings_warning {
        widgets::warning_banner(ui, warning);
    }

    ui.label(egui::RichText::new("アプリ").strong());
    egui::Grid::new("info_app")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("バージョン");
            ui.label(env!("CARGO_PKG_VERSION"));
            ui.end_row();
            ui.label("ビルド");
            ui.label(if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            });
            ui.end_row();
            ui.label("ライセンス");
            ui.label("GPL-3.0-or-later");
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("設定ファイル").strong());
    ui.horizontal(|ui| {
        ui.label("実効パス:");
        widgets::mono_path(ui, &app.config.path);
    });
    let config_state = match &app.config.state {
        ConfigState::Loaded => "正常",
        ConfigState::Missing => "未作成",
        ConfigState::Broken { .. } => "読込エラー",
    };
    ui.label(format!("状態: {config_state}"));
    if let ConfigState::Broken { error, .. } = &app.config.state {
        widgets::error_banner(ui, error);
    }
    egui::CollapsingHeader::new("候補パス").show(ui, |ui| {
        for candidate in &app.config.candidates {
            widgets::mono_path(ui, candidate);
        }
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("解像度スケール").strong());
    match &app.scale.snapshot.effective {
        Some(path) => {
            ui.horizontal(|ui| {
                ui.label("実効パス:");
                widgets::mono_path(ui, path);
            });
        }
        None => {
            ui.label("実効パス: なし");
        }
    }
    if let Some(value) = app.scale.snapshot.effective_value {
        ui.label(format!("現在値: {:.3}（{:.0}%）", value, value * 100.0));
    }
    egui::CollapsingHeader::new("候補パス").show(ui, |ui| {
        egui::Grid::new("info_scale_candidates")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for candidate in &app.scale.snapshot.candidates {
                    widgets::mono_path(ui, &candidate.path);
                    let state = match &candidate.state {
                        ScaleState::Valid { raw, value } => {
                            format!("有効: {raw} → {value:.3}")
                        }
                        ScaleState::Invalid { error } => format!("不正: {error}"),
                        ScaleState::Missing => "未作成".to_string(),
                    };
                    ui.label(state);
                    ui.end_row();
                }
            });
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("モデル").strong());
    ui.horizontal(|ui| {
        ui.label("既定パス:");
        widgets::mono_path(ui, &app.model.target);
    });
    let effective_model = match config::normalize_model_path(
        Some(&app.config.model_path_text),
        app.env.home.as_deref(),
    ) {
        Ok(Some(path)) => path,
        Ok(None) => app.model.target.clone(),
        Err(_) => app.model.target.clone(),
    };
    ui.horizontal(|ui| {
        ui.label("使用モデル:");
        widgets::mono_path(ui, &effective_model);
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("Vulkan レイヤー").strong());
    if let Some(paths) = &app.layer.snapshot.paths {
        ui.horizontal(|ui| {
            ui.label("manifest:");
            widgets::mono_path(ui, &paths.manifest);
        });
        ui.horizontal(|ui| {
            ui.label("ライブラリ:");
            widgets::mono_path(ui, &paths.lib);
        });
    }
    let layer_state = match &app.layer.snapshot.status {
        LayerStatus::Installed { .. } => "導入済み",
        LayerStatus::Partial { .. } => "不完全",
        LayerStatus::NotInstalled => "未導入",
        LayerStatus::Error { .. } => "エラー",
    };
    ui.label(format!("状態: {layer_state}"));

    ui.add_space(8.0);
    ui.label(egui::RichText::new("GUI 設定").strong());
    ui.horizontal(|ui| {
        widgets::mono_path(ui, &settings::path(&app.env));
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("環境値").strong());
    widgets::help_label(ui, "ゲーム側の環境と異なる場合があります。");
    egui::Grid::new("info_env")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for (label, value) in [
                ("HOME", app.env.home.as_ref()),
                ("XDG_CONFIG_HOME", app.env.xdg_config_home.as_ref()),
                ("XDG_RUNTIME_DIR", app.env.xdg_runtime_dir.as_ref()),
                ("XDG_DATA_HOME", app.env.xdg_data_home.as_ref()),
                (
                    "XRIZER_UPSCALER_CONFIG",
                    app.env.xrizer_upscaler_config.as_ref(),
                ),
                ("XRIZER_SCALE_FILE", app.env.xrizer_scale_file.as_ref()),
            ] {
                ui.label(label);
                match value {
                    Some(path) => widgets::mono_path(ui, path),
                    None => {
                        ui.label("(未設定)");
                    }
                }
                ui.end_row();
            }
        });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("同梱インストーラ").strong());
    widgets::help_label(
        ui,
        "XRizer main e6fbdd4 / GPL-3.0-or-later から同梱しています。",
    );
    widgets::help_label(ui, &format!("install-vulkan-layer.sh: {SCRIPT_SHA256}"));
    widgets::help_label(ui, &format!("manifest.json.in: {MANIFEST_SHA256}"));

    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button("設定ファイルをエディタで開く").clicked() {
            open_with_xdg(&app.config.path);
        }
        if let Some(parent) = app.config.path.parent()
            && ui.button("設定ディレクトリを開く").clicked()
        {
            open_with_xdg(parent);
        }
        if ui.button("GUI 設定を開く").clicked() {
            open_with_xdg(&settings::path(&app.env));
        }
    });
}

fn open_with_xdg(path: &Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
