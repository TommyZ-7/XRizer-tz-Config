//! Launch options tab.

use eframe::egui;

use crate::app::{App, ToastKind};
use crate::core::config::Preset;
use crate::core::launch::{self, CaptureMode, LaunchContext, LayerMode};
use crate::core::layer::LayerStatus;
use crate::ui::widgets;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    widgets::section_title(
        ui,
        "起動オプション",
        "これらの環境変数はゲーム起動時に読まれます。変更後はゲームを再起動してください。",
    );

    let mut changed = false;

    ui.label(egui::RichText::new("Vulkan レイヤー").strong());
    let previous_layer_mode = app.settings.launch.layer_mode;
    ui.horizontal_wrapped(|ui| {
        for (mode, label) in [
            (LayerMode::Unspecified, "指定しない"),
            (LayerMode::Enable, "有効 (XRIZER_UPSCALER_VK_LAYER=1)"),
            (
                LayerMode::Disable,
                "強制無効 (XRIZER_UPSCALER_VK_LAYER_DISABLE=1)",
            ),
        ] {
            if ui
                .radio_value(&mut app.settings.launch.layer_mode, mode, label)
                .changed()
            {
                changed = true;
            }
        }
    });
    if app.settings.launch.layer_mode != previous_layer_mode {
        changed = true;
        if app.settings.launch.layer_mode == LayerMode::Enable
            && matches!(&app.layer.snapshot.status, LayerStatus::Installed { .. })
        {
            app.settings.launch.pin_layer_path = true;
        }
    }
    if app.settings.launch.layer_mode == LayerMode::Enable
        && !matches!(&app.layer.snapshot.status, LayerStatus::Installed { .. })
    {
        widgets::warning_banner(
            ui,
            "Vulkan レイヤーが未導入です。ml は FSR にフォールバックします。",
        );
    }

    ui.add_space(8.0);
    ui.label(egui::RichText::new("学習用キャプチャ").strong());
    if ui
        .checkbox(
            &mut app.settings.launch.capture_enabled,
            "キャプチャを有効にする",
        )
        .changed()
    {
        changed = true;
    }
    ui.add_enabled_ui(app.settings.launch.capture_enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("保存先");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut app.settings.launch.capture_dir)
                        .desired_width(420.0),
                )
                .changed()
            {
                changed = true;
            }
            if ui.button("参照...").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                app.settings.launch.capture_dir = path.display().to_string();
                changed = true;
            }
        });
        if let Ok(expanded) = launch::expand_tilde(
            &app.settings.launch.capture_dir,
            app.env.home.as_deref(),
        ) && !expanded.is_empty()
        {
            widgets::help_label(ui, &format!("展開後: {expanded}"));
        }
        ui.horizontal_wrapped(|ui| {
            for (mode, label) in [
                (CaptureMode::Interval, "interval（一定間隔）"),
                (CaptureMode::Shutter, "shutter（コントローラ）"),
            ] {
                if ui
                    .radio_value(&mut app.settings.launch.capture_mode, mode, label)
                    .changed()
                {
                    changed = true;
                }
            }
        });
        egui::Grid::new("capture_grid")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                match app.settings.launch.capture_mode {
                    CaptureMode::Interval => {
                        ui.label("間隔");
                        if ui
                            .add(
                                egui::DragValue::new(&mut app.settings.launch.interval_ms)
                                    .range(100..=60_000)
                                    .suffix(" ms"),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.end_row();
                    }
                    CaptureMode::Shutter => {
                        ui.label("長押し");
                        if ui
                            .add(
                                egui::DragValue::new(&mut app.settings.launch.hold_ms)
                                    .range(100..=500)
                                    .suffix(" ms"),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.end_row();
                        ui.label("ロックアウト");
                        if ui
                            .add(
                                egui::DragValue::new(&mut app.settings.launch.lockout_ms)
                                    .range(200..=5_000)
                                    .suffix(" ms"),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        ui.end_row();
                    }
                }
                ui.label("最大ペア数");
                if ui
                    .add(
                        egui::DragValue::new(&mut app.settings.launch.max_pairs)
                            .range(1..=100_000),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.end_row();
                ui.label("シーン名");
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.settings.launch.scene)
                            .desired_width(300.0)
                            .hint_text("未指定"),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.end_row();
            });
        match app.settings.launch.capture_mode {
            CaptureMode::Interval => widgets::help_label(
                ui,
                "一定間隔で眼テクスチャを PNG に保存します。ゲーム入力には依存しません。",
            ),
            CaptureMode::Shutter => widgets::help_label(
                ui,
                "両手のグリップ + トリガーを同時に保持すると 1 ペア撮影します。入力はゲームにもそのまま伝わります。",
            ),
        }
    });

    ui.add_space(8.0);
    ui.label(egui::RichText::new("上級者向け: パスの固定").strong());
    if ui
        .checkbox(
            &mut app.settings.launch.pin_config_path,
            "設定ファイルのパスを固定する (XRIZER_UPSCALER_CONFIG)",
        )
        .changed()
    {
        changed = true;
    }
    let can_pin_scale = app.scale.snapshot.effective.is_some();
    if ui
        .add_enabled(
            can_pin_scale,
            egui::Checkbox::new(
                &mut app.settings.launch.pin_scale_path,
                "スケールファイルのパスを固定する (XRIZER_SCALE_FILE)",
            ),
        )
        .changed()
    {
        changed = true;
    }
    let can_pin_layer =
        app.settings.launch.layer_mode == LayerMode::Enable && app.layer.snapshot.paths.is_some();
    if ui
        .add_enabled(
            can_pin_layer,
            egui::Checkbox::new(
                &mut app.settings.launch.pin_layer_path,
                "レイヤーの探索パスを固定する (VK_ADD_IMPLICIT_LAYER_PATH)",
            ),
        )
        .changed()
    {
        changed = true;
    }
    widgets::help_label(
        ui,
        "Steam Linux Runtime などでゲーム側の HOME / XDG_* が GUI と違う場合に、同じファイル / manifest を読ませます。Vulkan loader 1.3.296+ が必要です。",
    );

    let output = {
        let context = build_context(app);
        launch::generate(&app.settings.launch, &context)
    };
    app.launch_output = output;

    for error in &app.launch_output.errors {
        widgets::error_banner(ui, error);
    }
    for warning in &app.launch_output.warnings {
        widgets::warning_banner(ui, warning);
    }

    ui.add_space(8.0);
    ui.label(egui::RichText::new("生成された Steam 起動オプション").strong());
    let mut command = app.launch_output.command.clone();
    ui.add(
        egui::TextEdit::multiline(&mut command)
            .desired_width(f32::INFINITY)
            .interactive(false)
            .font(egui::TextStyle::Monospace),
    );
    let mut copy_command = false;
    let mut copy_env = false;
    ui.horizontal(|ui| {
        let enabled = app.launch_output.errors.is_empty();
        if ui
            .add_enabled(enabled, egui::Button::new("コピー"))
            .clicked()
        {
            copy_command = true;
        }
        if ui
            .add_enabled(enabled, egui::Button::new("環境変数だけコピー"))
            .clicked()
        {
            copy_env = true;
        }
        widgets::help_label(ui, "%command% を末尾に含みます。");
    });

    if changed {
        app.settings_dirty = true;
    }
    if copy_command {
        ui.ctx().copy_text(app.launch_output.command.clone());
        app.show_toast(ToastKind::Success, "起動オプションをコピーしました");
    }
    if copy_env {
        ui.ctx().copy_text(app.launch_output.env_only.clone());
        app.show_toast(ToastKind::Success, "環境変数をコピーしました");
    }
}

fn build_context(app: &App) -> LaunchContext<'_> {
    LaunchContext {
        env: &app.env,
        config_path: &app.config.path,
        scale_path: app.scale.snapshot.effective.as_deref(),
        layer_manifest_dir: app
            .layer
            .snapshot
            .paths
            .as_ref()
            .map(|paths| paths.manifest_dir.as_path()),
        layer_installed: matches!(&app.layer.snapshot.status, LayerStatus::Installed { .. }),
        preset_is_ml: app.config.form.preset == Preset::Ml,
    }
}
