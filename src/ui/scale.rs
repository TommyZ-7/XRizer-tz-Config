//! Resolution scale tab.

use eframe::egui;

use crate::app::{App, ToastKind};
use crate::core::scale::{self, EffectiveSource, ScaleState};
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_title(
        ui,
        "解像度スケール",
        "ゲームが描画する解像度の倍率です。保存すると本体が 500ms 以内に読み込みます。",
    );

    if let Some(error) = &app.scale.error {
        widgets::error_banner(ui, error);
    }
    if let Some(message) = &app.scale.save_message {
        ui.label(egui::RichText::new(message).color(egui::Color32::from_rgb(130, 210, 140)));
    }

    let mut force_save = false;
    let mut reload = false;
    if app.scale.conflict {
        widgets::warning_banner(
            ui,
            "スケールファイルが外部で変更されました。未保存の変更があります。",
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

    let snapshot = app.scale.snapshot.clone();
    for notice in &snapshot.notices {
        widgets::warning_banner(ui, notice);
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add(
            egui::Slider::new(&mut app.scale.form_value, 0.1..=4.0)
                .max_decimals(3)
                .text("倍率"),
        );
        ui.add(
            egui::DragValue::new(&mut app.scale.form_value)
                .speed(0.005)
                .range(0.1..=4.0)
                .max_decimals(3),
        );
        ui.label(format!("{:.0}%", app.scale.form_value * 100.0));
    });
    ui.horizontal_wrapped(|ui| {
        for (label, value) in [
            ("50%", 0.5),
            ("70%", 0.7),
            ("85%", 0.85),
            ("100%", 1.0),
            ("150%", 1.5),
        ] {
            if ui.button(label).clicked() {
                app.scale.form_value = value;
            }
        }
    });
    widgets::help_label(
        ui,
        "推奨開始点は 70%。ゲームが 70% で描画し、XRizer がランタイム推奨サイズへ拡大します。",
    );

    ui.add_space(8.0);
    ui.label(egui::RichText::new("実効パス").strong());
    match &snapshot.effective {
        Some(path) => {
            widgets::mono_path(ui, path);
            match snapshot.source {
                Some(EffectiveSource::Existing) => {
                    widgets::help_label(ui, "既存の有効なファイルに書き込みます。");
                }
                Some(EffectiveSource::NewFile) => {
                    widgets::help_label(ui, "このパスを新規作成します。");
                }
                None => {}
            }
            if !scale::is_persistent(path, &app.env) {
                widgets::warning_banner(
                    ui,
                    "このパスはログアウト / 再起動で消える可能性があります。",
                );
            }
        }
        None => {
            widgets::error_banner(
                ui,
                "有効なスケールファイルがなく、既存ファイルはすべて不正です。保存できません。",
            );
        }
    }
    if let Some(value) = snapshot.effective_value {
        ui.label(format!("現在値: {:.3}（{:.0}%）", value, value * 100.0));
    }
    if let Some((path, value)) = &snapshot.persistent_fallback {
        ui.label(format!(
            "ログアウト後は {} の {:.3} が使われます",
            path.display(),
            value
        ));
    }

    if app.config.form.auto_scale {
        widgets::warning_banner(
            ui,
            "auto_scale が有効です。本体がこのファイルを 0.05 刻みで書き換えるため、手動保存は次の調整で上書きされる可能性があります。",
        );
    }

    ui.add_space(8.0);
    egui::CollapsingHeader::new(format!("候補パス ({})", snapshot.candidates.len()))
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("scale_candidates")
                .num_columns(4)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("パス");
                    ui.label("状態");
                    ui.label("書込");
                    ui.label("永続");
                    ui.end_row();
                    for candidate in &snapshot.candidates {
                        widgets::mono_path(ui, &candidate.path);
                        let state = match &candidate.state {
                            ScaleState::Valid { raw, value } => {
                                format!("有効: {raw} → {value:.3}")
                            }
                            ScaleState::Invalid { error } => format!("不正: {error}"),
                            ScaleState::Missing => "未作成".to_string(),
                        };
                        ui.label(state);
                        ui.label(if candidate.writable { "可" } else { "不可" });
                        ui.label(if candidate.persistent {
                            "永続"
                        } else {
                            "揮発"
                        });
                        ui.end_row();
                    }
                });
        });

    app.scale.recompute_dirty();

    ui.add_space(12.0);
    ui.separator();
    let mut save_clicked = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                snapshot.effective.is_some(),
                egui::Button::new("スケールを保存"),
            )
            .clicked()
        {
            save_clicked = true;
        }
        if ui.button("再読込").clicked() {
            reload = true;
        }
    });

    if force_save {
        if app.scale.save(&app.env, true) {
            let message = app
                .scale
                .save_message
                .clone()
                .unwrap_or_else(|| "スケールを保存しました".to_string());
            app.show_toast(ToastKind::Success, message);
        } else {
            let error = app
                .scale
                .error
                .clone()
                .unwrap_or_else(|| "スケールを保存できませんでした".to_string());
            app.show_toast(ToastKind::Error, error);
        }
    }
    if reload {
        app.scale.reload(&app.env);
    }
    if save_clicked {
        app.save_all();
    }
}
