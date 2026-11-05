//! Upscaler settings tab.

use eframe::egui;

use crate::app::{App, ConfigState, ToastKind};
use crate::core::config::{self, KNOWN_KEYS, Preset, UpscalerConfig};
use crate::core::layer::LayerStatus;
use crate::core::model;
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_title(
        ui,
        "アップスケーラ",
        "変更は「設定を保存」または Ctrl+S で upscaler.json に書き込まれ、ゲームが 500ms 以内に読み込みます。",
    );

    let state = app.config.state.clone();
    if let ConfigState::Broken { error, raw_text } = &state {
        widgets::error_banner(ui, &format!("設定ファイルを読み込めません: {error}"));
        widgets::help_label(ui, &format!("対象: {}", app.config.path.display()));
        ui.label("このファイルは上書きしません。修正して再読み込みしてください。");
        let mut reload = false;
        let mut open = false;
        ui.horizontal(|ui| {
            if ui.button("再読み込み").clicked() {
                reload = true;
            }
            if ui.button("エディタで開く").clicked() {
                open = true;
            }
        });
        let preview = raw_text.chars().take(64 * 1024).collect::<String>();
        egui::CollapsingHeader::new("生ファイルを表示（先頭 64 KiB）").show(ui, |ui| {
            let mut preview = preview;
            ui.add(
                egui::TextEdit::multiline(&mut preview)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        });
        if reload {
            app.config.reload(&app.env);
        }
        if open {
            open_in_editor(&app.config.path);
        }
        return;
    }

    if matches!(&state, ConfigState::Missing) {
        widgets::warning_banner(ui, "設定ファイルはまだありません。保存すると作成します。");
    }
    for notice in &app.config.notices {
        widgets::warning_banner(ui, notice);
    }
    if let Some(error) = &app.config.error {
        widgets::error_banner(ui, error);
    }
    if let Some(message) = &app.config.save_message {
        ui.label(egui::RichText::new(message).color(egui::Color32::from_rgb(130, 210, 140)));
    }

    let mut force_save = false;
    let mut reload = false;
    if app.config.conflict {
        widgets::warning_banner(
            ui,
            "設定ファイルが外部で変更されました。未保存の変更があります。",
        );
        ui.horizontal(|ui| {
            if ui.button("このまま保存（上書き）").clicked() {
                force_save = true;
            }
            if ui.button("再読み込み（変更を破棄）").clicked() {
                reload = true;
            }
        });
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.config.form.enabled, "アップスケーラを有効にする");
        widgets::help_label(ui, "ゲームのプロジェクションレイヤーだけを処理します。");
    });

    ui.add_space(4.0);
    ui.label(egui::RichText::new("プリセット").strong());
    ui.horizontal_wrapped(|ui| {
        for (preset, description) in [
            (Preset::Fast, "バイリニア + 簡易シャープ。最も軽い。"),
            (Preset::Balanced, "SGSR 1 + 適応シャープ。既定。"),
            (
                Preset::Fsr,
                "FSR 1 EASU + RCAS。AA / トーンマップ済み入力向け。",
            ),
            (
                Preset::Ml,
                "FSR 1 + 学習モデル。モデルと DP4A / Vulkan レイヤーが必要。",
            ),
        ] {
            ui.group(|ui| {
                ui.vertical(|ui| {
                    ui.selectable_value(&mut app.config.form.preset, preset, preset.label());
                    ui.set_max_width(220.0);
                    widgets::help_label(ui, description);
                });
            });
        }
    });
    match app.config.form.preset {
        Preset::Ml => {
            if !matches!(app.layer.snapshot.status, LayerStatus::Installed { .. }) {
                widgets::warning_banner(
                    ui,
                    "ml には Vulkan レイヤーの導入と有効化が必要です。未導入の場合は fsr にフォールバックします。",
                );
            }
            let model_available = match config::normalize_model_path(
                Some(&app.config.model_path_text),
                app.env.home.as_deref(),
            ) {
                Ok(Some(path)) => path.is_file(),
                Ok(None) => model::default_model_path(&app.env).is_file(),
                Err(_) => false,
            };
            if !model_available {
                widgets::warning_banner(
                    ui,
                    "使用するモデルがありません。ml は fsr にフォールバックします。",
                );
            }
        }
        Preset::Balanced => {
            widgets::help_label(ui, "反転 bounds の入力では fast にフォールバックします。");
        }
        _ => {}
    }

    ui.add_space(8.0);
    ui.label(egui::RichText::new("強調の重み場").strong());
    ui.horizontal_top(|ui| {
        egui::Grid::new("weight_grid")
            .num_columns(3)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                slider_row(
                    ui,
                    "中心 X",
                    &mut app.config.form.center[0],
                    0.0..=1.0,
                    0.01,
                    "0–1。歪み補正前の眼テクスチャ UV。",
                );
                slider_row(
                    ui,
                    "中心 Y",
                    &mut app.config.form.center[1],
                    0.0..=1.0,
                    0.01,
                    "0–1。",
                );
                slider_row(
                    ui,
                    "radius",
                    &mut app.config.form.radius,
                    0.05..=2.0,
                    0.01,
                    "重みが最大になる半径。",
                );
                slider_row(
                    ui,
                    "feather",
                    &mut app.config.form.feather,
                    0.0..=1.0,
                    0.01,
                    "radius の外側で重みが 0 へ落ちる幅。",
                );
                slider_row(
                    ui,
                    "strength",
                    &mut app.config.form.strength,
                    0.0..=3.0,
                    0.01,
                    "ML 残差とシャープニングの強さ。基礎リサンプルには影響しません。",
                );
            });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            weight_preview(ui, &app.config.form);
            widgets::help_label(ui, "正方形・アスペクト補正なしの簡易表示。");
        });
    });
    ui.horizontal(|ui| {
        if ui.button("中央に戻す").clicked() {
            app.config.form.center = [0.5, 0.5];
        }
        widgets::help_label(ui, "radius=2.0 / feather=0.0 で実質全面になります。");
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("出力と表示").strong());
    egui::Grid::new("output_grid")
        .num_columns(3)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            slider_row(
                ui,
                "output_scale",
                &mut app.config.form.output_scale,
                0.5..=2.0,
                0.01,
                "ランタイム推奨サイズへの倍率。",
            );
            checkbox_row(
                ui,
                "preserve_alpha",
                &mut app.config.form.preserve_alpha,
                "半透明ピクセルを強調しない",
                "alpha < 0.999 のピクセルをそのまま保持します。",
            );
            checkbox_row(
                ui,
                "compare",
                &mut app.config.form.compare,
                "A/B 比較",
                "2 秒ごとに強調の有効 / 無効を切り替えます。基礎リサンプルは常に有効です。",
            );
            checkbox_row(
                ui,
                "show_region",
                &mut app.config.form.show_region,
                "radius の等高線を表示",
                "調整用です。ゲーム内に重み最大の境界を描きます。",
            );
        });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("自動スケール").strong());
    ui.checkbox(
        &mut app.config.form.auto_scale,
        "フレーム時間に応じて xrizer_scale を自動調整する",
    );
    widgets::help_label(
        ui,
        "有効にすると本体が xrizer_scale を 0.05 刻みで書き換えます。",
    );
    egui::Grid::new("auto_scale_grid")
        .num_columns(3)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            slider_row(
                ui,
                "min_scale",
                &mut app.config.form.min_scale,
                0.1..=4.0,
                0.01,
                "自動調整の下限。",
            );
            slider_row(
                ui,
                "max_scale",
                &mut app.config.form.max_scale,
                0.1..=4.0,
                0.01,
                "自動調整の上限。min > max の場合は本体が入れ替えて解釈します。",
            );
        });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("モデル").strong());
    ui.horizontal(|ui| {
        ui.label("model_path");
        ui.add(
            egui::TextEdit::singleline(&mut app.config.model_path_text)
                .desired_width(420.0)
                .hint_text("null（既定）"),
        );
        if ui.button("クリア").clicked() {
            app.config.model_path_text.clear();
        }
    });
    match config::normalize_model_path(Some(&app.config.model_path_text), app.env.home.as_deref()) {
        Ok(Some(path)) => {
            ui.label(format!("使用モデル: {}", path.display()));
            let default = model::default_model_path(&app.env);
            if path != default {
                widgets::warning_banner(
                    ui,
                    "既定パスへの配置は使われません。モデルタブで model_path を null に戻せます。",
                );
            }
        }
        Ok(None) => {
            widgets::help_label(
                ui,
                "既定の ~/.config/xrizer/upscaler_model.xru を使用します。",
            );
        }
        Err(error) => widgets::error_banner(ui, &error),
    }

    ui.add_space(8.0);
    if ui.button("このタブを既定値に戻す").clicked() {
        app.config.form = UpscalerConfig::default();
        app.config.model_path_text.clear();
    }

    let unknown: serde_json::Map<String, serde_json::Value> = app
        .config
        .raw
        .iter()
        .filter(|(key, _)| !KNOWN_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if !unknown.is_empty() {
        egui::CollapsingHeader::new(format!(
            "未知のキー ({}) はそのまま保持されます",
            unknown.len()
        ))
        .show(ui, |ui| {
            let text = serde_json::to_string_pretty(&unknown).unwrap_or_default();
            let mut text = text;
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        });
    }

    app.config.recompute_dirty();

    ui.add_space(12.0);
    ui.separator();
    let mut save_clicked = false;
    ui.horizontal(|ui| {
        if ui.button("設定を保存").clicked() {
            save_clicked = true;
        }
        widgets::help_label(ui, "Ctrl+S でも保存できます。");
    });

    if force_save {
        if app.config.save(&app.env, true) {
            let message = app
                .config
                .save_message
                .clone()
                .unwrap_or_else(|| "設定を保存しました".to_string());
            app.show_toast(ToastKind::Success, message);
        } else {
            let error = app
                .config
                .error
                .clone()
                .unwrap_or_else(|| "設定を保存できませんでした".to_string());
            app.show_toast(ToastKind::Error, error);
        }
    }
    if reload {
        app.config.reload(&app.env);
    }
    if save_clicked {
        app.save_all();
    }
}

fn slider_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    speed: f32,
    help: &str,
) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(value, range.clone()).max_decimals(3));
        ui.add(egui::DragValue::new(value).speed(speed).range(range));
    });
    widgets::help_label(ui, help);
    ui.end_row();
}

fn checkbox_row(ui: &mut egui::Ui, label: &str, value: &mut bool, text: &str, help: &str) {
    ui.label(label);
    ui.checkbox(value, text);
    widgets::help_label(ui, help);
    ui.end_row();
}

fn weight_preview(ui: &mut egui::Ui, form: &UpscalerConfig) {
    let size = egui::Vec2::splat(160.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, egui::Color32::from_rgb(28, 30, 34));
    let center = rect.min
        + egui::vec2(
            form.center[0] * rect.width(),
            form.center[1] * rect.height(),
        );
    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(150, 190, 240));
    painter.circle_stroke(center, form.radius * rect.width(), stroke);
    painter.circle_stroke(
        center,
        (form.radius + form.feather) * rect.width(),
        egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 110, 140)),
    );
    let cross = 6.0;
    painter.line_segment(
        [
            center - egui::vec2(cross, 0.0),
            center + egui::vec2(cross, 0.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            center - egui::vec2(0.0, cross),
            center + egui::vec2(0.0, cross),
        ],
        stroke,
    );
}

fn open_in_editor(path: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
