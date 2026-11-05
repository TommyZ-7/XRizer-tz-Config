//! Application state and top-level eframe shell.

// Temporary until every field and action is wired in (Task 17).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::core::config::{self, UpscalerConfig};
use crate::core::env::{self, Env};
use crate::core::launch::LaunchOutput;
use crate::core::layer::{self, InstallResult, LayerSnapshot, SoInfo};
use crate::core::model::{self, ModelInfo, ModelInstallOutcome};
use crate::core::scale::{self, ScaleSnapshot};
use crate::core::settings::{self, AppSettings};
use crate::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Upscaler,
    Scale,
    Model,
    Layer,
    Launch,
    Info,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Self::Upscaler => "アップスケーラ",
            Self::Scale => "解像度スケール",
            Self::Model => "モデル",
            Self::Layer => "Vulkan レイヤー",
            Self::Launch => "起動オプション",
            Self::Info => "情報",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Upscaler => "upscaler",
            Self::Scale => "scale",
            Self::Model => "model",
            Self::Layer => "layer",
            Self::Launch => "launch",
            Self::Info => "info",
        }
    }

    pub fn from_key(key: Option<&str>) -> Self {
        match key {
            Some("scale") => Self::Scale,
            Some("model") => Self::Model,
            Some("layer") => Self::Layer,
            Some("launch") => Self::Launch,
            Some("info") => Self::Info,
            _ => Self::Upscaler,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub until: Instant,
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    ReloadAll,
    InstallModel { source: PathBuf, target: PathBuf },
    RemoveLayer,
    OverwriteScale,
    OverwriteConfig,
}

#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone)]
pub enum ConfigState {
    Loaded,
    Missing,
    Broken { error: String, raw_text: String },
}

pub struct ConfigView {
    pub path: PathBuf,
    pub candidates: Vec<PathBuf>,
    pub state: ConfigState,
    pub form: UpscalerConfig,
    pub model_path_text: String,
    pub saved_form: UpscalerConfig,
    pub saved_model_path_text: String,
    pub raw: serde_json::Map<String, serde_json::Value>,
    pub notices: Vec<String>,
    pub dirty: bool,
    pub fingerprint: Option<u64>,
    pub conflict: bool,
    pub error: Option<String>,
    pub save_message: Option<String>,
}

impl ConfigView {
    pub fn new(env: &Env) -> Self {
        let (path, candidates) = match config::resolve_path(env) {
            Ok(info) => (info.path, info.candidates),
            Err(error) => {
                let candidates = env::config_candidates(env);
                let path = candidates
                    .first()
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from("/tmp/xrizer_upscaler.json"));
                return Self {
                    path,
                    candidates,
                    state: ConfigState::Broken {
                        error,
                        raw_text: String::new(),
                    },
                    form: UpscalerConfig::default(),
                    model_path_text: String::new(),
                    saved_form: UpscalerConfig::default(),
                    saved_model_path_text: String::new(),
                    raw: serde_json::Map::new(),
                    notices: Vec::new(),
                    dirty: false,
                    fingerprint: None,
                    conflict: false,
                    error: None,
                    save_message: None,
                };
            }
        };
        let mut view = Self {
            path,
            candidates,
            state: ConfigState::Missing,
            form: UpscalerConfig::default(),
            model_path_text: String::new(),
            saved_form: UpscalerConfig::default(),
            saved_model_path_text: String::new(),
            raw: serde_json::Map::new(),
            notices: Vec::new(),
            dirty: false,
            fingerprint: None,
            conflict: false,
            error: None,
            save_message: None,
        };
        view.reload(env);
        view
    }

    pub fn reload(&mut self, _env: &Env) {
        match config::load(&self.path) {
            Ok(loaded) => {
                self.state = if loaded.existed {
                    ConfigState::Loaded
                } else {
                    ConfigState::Missing
                };
                self.form = loaded.value.clone();
                self.model_path_text = loaded
                    .value
                    .model_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default();
                self.saved_form = loaded.value;
                self.saved_model_path_text = self.model_path_text.clone();
                self.raw = loaded.raw;
                self.notices = loaded.notices;
                self.dirty = false;
                self.fingerprint = config::file_fingerprint(&self.path);
                self.conflict = false;
                self.error = None;
            }
            Err(broken) => {
                self.state = ConfigState::Broken {
                    error: broken.error,
                    raw_text: broken.raw_text,
                };
                self.notices.clear();
                self.dirty = false;
                self.fingerprint = config::file_fingerprint(&self.path);
                self.conflict = false;
                self.error = None;
            }
        }
    }

    pub fn recompute_dirty(&mut self) {
        self.dirty =
            self.form != self.saved_form || self.model_path_text != self.saved_model_path_text;
    }

    pub fn save(&mut self, env: &Env, force: bool) -> bool {
        if matches!(&self.state, ConfigState::Broken { .. }) {
            self.error = Some(
                "設定ファイルが壊れているため保存できません。修正して再読み込みしてください"
                    .to_string(),
            );
            return false;
        }
        let normalized =
            match config::normalize_model_path(Some(&self.model_path_text), env.home.as_deref()) {
                Ok(path) => path,
                Err(error) => {
                    self.error = Some(error);
                    return false;
                }
            };
        let mut value = self.form.clone();
        value.model_path = normalized;
        if !force {
            let current = config::file_fingerprint(&self.path);
            if current != self.fingerprint {
                self.conflict = true;
                self.error = Some(
                    "設定ファイルが外部で変更されました。再読み込みするか、上書き保存してください"
                        .to_string(),
                );
                return false;
            }
        }
        match config::save(&self.path, &self.raw, &value) {
            Ok(outcome) => {
                self.reload(env);
                self.save_message = Some(match outcome.backup {
                    Some(backup) => {
                        format!("保存しました（バックアップ: {}）", backup.display())
                    }
                    None => "保存しました".to_string(),
                });
                true
            }
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }
}

pub struct ScaleView {
    pub snapshot: ScaleSnapshot,
    pub form_value: f32,
    pub saved_value: f32,
    pub dirty: bool,
    pub loaded_fingerprint: u64,
    pub conflict: bool,
    pub error: Option<String>,
    pub save_message: Option<String>,
}

impl ScaleView {
    pub fn new(env: &Env) -> Self {
        let snapshot = scale::snapshot(env);
        let form_value = snapshot.effective_value.unwrap_or(1.0);
        Self {
            loaded_fingerprint: snapshot.fingerprint,
            snapshot,
            form_value,
            saved_value: form_value,
            dirty: false,
            conflict: false,
            error: None,
            save_message: None,
        }
    }

    pub fn reload(&mut self, env: &Env) {
        self.snapshot = scale::snapshot(env);
        self.form_value = self.snapshot.effective_value.unwrap_or(1.0);
        self.saved_value = self.form_value;
        self.loaded_fingerprint = self.snapshot.fingerprint;
        self.dirty = false;
        self.conflict = false;
        self.error = None;
    }

    pub fn recompute_dirty(&mut self) {
        self.dirty = (self.form_value - self.saved_value).abs() > 0.0005;
    }

    pub fn save(&mut self, env: &Env, force: bool) -> bool {
        let expected = if force {
            None
        } else {
            Some(self.loaded_fingerprint)
        };
        match scale::save(env, self.form_value, expected) {
            Ok(outcome) => {
                self.reload(env);
                self.save_message = Some(match outcome.backup {
                    Some(backup) => {
                        format!("保存しました（バックアップ: {}）", backup.display())
                    }
                    None => "保存しました".to_string(),
                });
                true
            }
            Err(scale::ScaleSaveError::Conflict {
                path,
                current_value,
            }) => {
                self.conflict = true;
                self.error = Some(format!(
                    "外部で {} が {:.3} に変更されました。再読み込みするか、上書き保存してください",
                    path.display(),
                    current_value
                ));
                false
            }
            Err(scale::ScaleSaveError::NoTarget { reason }) => {
                self.error = Some(format!("保存先がありません: {reason}"));
                false
            }
            Err(scale::ScaleSaveError::Io(error)) => {
                self.error = Some(error);
                false
            }
        }
    }
}

pub struct ModelView {
    pub target: PathBuf,
    pub current: Option<Result<ModelInfo, String>>,
    pub source_text: String,
    pub source: Option<Result<ModelInfo, String>>,
    pub busy: Option<Receiver<Result<ModelInstallOutcome, String>>>,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl ModelView {
    pub fn new(env: &Env) -> Self {
        let target = model::default_model_path(env);
        let current = if target.exists() {
            Some(model::inspect(&target))
        } else {
            None
        };
        Self {
            target,
            current,
            source_text: String::new(),
            source: None,
            busy: None,
            error: None,
            message: None,
        }
    }

    pub fn refresh_current(&mut self, path: &Path) {
        self.current = if path.exists() {
            Some(model::inspect(path))
        } else {
            None
        };
    }

    pub fn refresh_source(&mut self) {
        let text = self.source_text.trim();
        self.source = if text.is_empty() {
            None
        } else {
            Some(model::inspect(Path::new(text)))
        };
    }
}

pub enum LayerBusy {
    Install(Receiver<Result<InstallResult, String>>),
    Remove(Receiver<Result<InstallResult, String>>),
}

pub struct LayerView {
    pub so_text: String,
    pub so: Option<Result<SoInfo, String>>,
    pub snapshot: LayerSnapshot,
    pub busy: Option<LayerBusy>,
    pub log: String,
    pub error: Option<String>,
}

impl LayerView {
    pub fn new(env: &Env, settings: &AppSettings) -> Self {
        let so_text = settings
            .layer_so_path
            .clone()
            .or_else(|| layer::auto_detect_so(env).map(|path| path.display().to_string()))
            .unwrap_or_default();
        let so = if so_text.trim().is_empty() {
            None
        } else {
            Some(layer::inspect_so(Path::new(&so_text)))
        };
        Self {
            so_text,
            so,
            snapshot: layer::snapshot(env),
            busy: None,
            log: String::new(),
            error: None,
        }
    }

    pub fn refresh(&mut self, env: &Env) {
        self.snapshot = layer::snapshot(env);
    }

    pub fn refresh_so(&mut self) {
        let text = self.so_text.trim();
        self.so = if text.is_empty() {
            None
        } else {
            Some(layer::inspect_so(Path::new(text)))
        };
    }
}

pub struct App {
    pub env: Env,
    pub active_tab: Tab,
    pub config: ConfigView,
    pub scale: ScaleView,
    pub model: ModelView,
    pub layer: LayerView,
    pub settings: AppSettings,
    pub settings_dirty: bool,
    pub settings_warning: Option<String>,
    pub launch_output: LaunchOutput,
    pub toast: Option<Toast>,
    pub confirm: Option<ConfirmDialog>,
    pub last_poll: Instant,
    pub last_settings_save: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::theme::apply(&cc.egui_ctx);
        let env = Env::from_process();
        let (mut settings, mut settings_warning) = settings::load(&env);
        let mut settings_dirty = false;

        if settings.launch.capture_dir.is_empty()
            && let Some(home) = &env.home
        {
            settings.launch.capture_dir = home.join("xrizer-captures").display().to_string();
            settings_dirty = true;
        }
        let notices = settings.launch.sanitize();
        if !notices.is_empty() {
            settings_dirty = true;
            let joined = notices.join(" / ");
            settings_warning = Some(match settings_warning {
                Some(warning) => format!("{warning} / {joined}"),
                None => joined,
            });
        }

        let active_tab = Tab::from_key(settings.active_tab.as_deref());
        let config = ConfigView::new(&env);
        let scale = ScaleView::new(&env);
        let model = ModelView::new(&env);
        let layer = LayerView::new(&env, &settings);

        Self {
            env,
            active_tab,
            config,
            scale,
            model,
            layer,
            settings,
            settings_dirty,
            settings_warning,
            launch_output: LaunchOutput {
                command: "%command%".to_string(),
                env_only: String::new(),
                errors: Vec::new(),
                warnings: Vec::new(),
            },
            toast: None,
            confirm: None,
            last_poll: Instant::now(),
            last_settings_save: Instant::now(),
        }
    }

    pub fn start_model_install(&mut self, source: PathBuf, target: PathBuf) {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = model::install(&source, &target);
            let _ = sender.send(result);
        });
        self.model.busy = Some(receiver);
        self.model.error = None;
        self.model.message = None;
    }

    pub fn save_all(&mut self) {
        let mut saved = Vec::new();
        let mut errors = Vec::new();

        if matches!(&self.config.state, ConfigState::Broken { .. }) {
            errors.push("設定ファイルが壊れているため保存できません".to_string());
        } else if self.config.dirty {
            if self.config.save(&self.env, false) {
                saved.push("設定".to_string());
            } else {
                errors.push(format!(
                    "設定: {}",
                    self.config
                        .error
                        .clone()
                        .unwrap_or_else(|| "保存できませんでした".to_string())
                ));
            }
        }

        if self.scale.dirty {
            if self.scale.save(&self.env, false) {
                saved.push("スケール".to_string());
            } else {
                errors.push(format!(
                    "スケール: {}",
                    self.scale
                        .error
                        .clone()
                        .unwrap_or_else(|| "保存できませんでした".to_string())
                ));
            }
        }

        if !errors.is_empty() {
            self.show_toast(ToastKind::Error, errors.join(" / "));
        } else if saved.is_empty() {
            self.show_toast(ToastKind::Info, "変更はありません");
        } else {
            self.show_toast(
                ToastKind::Success,
                format!("保存しました: {}", saved.join(" / ")),
            );
        }
    }

    pub fn reload_all(&mut self) {
        if self.config.dirty || self.scale.dirty {
            self.confirm = Some(ConfirmDialog {
                title: "再読み込み".to_string(),
                body: "未保存の変更を破棄して再読み込みしますか？".to_string(),
                confirm_label: "再読み込み".to_string(),
                action: ConfirmAction::ReloadAll,
            });
        } else {
            self.reload_all_now();
        }
    }

    fn reload_all_now(&mut self) {
        self.config.reload(&self.env);
        self.scale.reload(&self.env);
        self.show_toast(ToastKind::Info, "再読み込みしました");
    }

    pub fn poll_files(&mut self) {}

    pub fn poll_background(&mut self) {
        let model_result = self
            .model
            .busy
            .as_ref()
            .map(std::sync::mpsc::Receiver::try_recv);
        match model_result {
            Some(Ok(result)) => {
                self.model.busy = None;
                match result {
                    Ok(outcome) => {
                        let target = self.model.target.clone();
                        self.model.refresh_current(&target);
                        self.model.message = Some(match outcome.backup {
                            Some(backup) => {
                                format!("配置しました（バックアップ: {}）", backup.display())
                            }
                            None => "配置しました".to_string(),
                        });
                        self.show_toast(ToastKind::Success, "モデルを配置しました");
                    }
                    Err(error) => {
                        self.model.error = Some(error.clone());
                        self.show_toast(ToastKind::Error, error);
                    }
                }
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {}
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.model.busy = None;
                self.model.error = Some("モデル配置スレッドが終了しました".to_string());
            }
            None => {}
        }
    }

    pub fn apply_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::ReloadAll => self.reload_all_now(),
            ConfirmAction::InstallModel { source, target } => {
                self.start_model_install(source, target);
            }
            _ => self.show_toast(ToastKind::Info, "この操作は次のタスクで実装されます"),
        }
    }

    pub fn show_toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            until: Instant::now() + Duration::from_secs(3),
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let mut save_clicked = false;
        let mut reload_clicked = false;
        ui.horizontal(|ui| {
            ui.heading("XRizer-tz Config");
            ui.separator();
            let (config_text, config_color) = match &self.config.state {
                ConfigState::Loaded => ("設定: 正常", egui::Color32::from_rgb(130, 210, 140)),
                ConfigState::Missing => ("設定: 未作成", egui::Color32::GRAY),
                ConfigState::Broken { .. } => {
                    ("設定: 読込エラー", egui::Color32::from_rgb(240, 110, 110))
                }
            };
            ui::widgets::status_badge(ui, config_text, config_color);

            let (scale_text, scale_color) = match self.scale.snapshot.effective_value {
                Some(value) => (
                    format!("スケール: {:.0}%", value * 100.0),
                    egui::Color32::from_rgb(150, 190, 240),
                ),
                None => ("スケール: なし".to_string(), egui::Color32::GRAY),
            };
            ui::widgets::status_badge(ui, &scale_text, scale_color);

            let (layer_text, layer_color) = match &self.layer.snapshot.status {
                layer::LayerStatus::Installed { .. } => {
                    ("レイヤー: 導入済", egui::Color32::from_rgb(130, 210, 140))
                }
                layer::LayerStatus::Partial { .. } => {
                    ("レイヤー: 不完全", egui::Color32::from_rgb(230, 180, 70))
                }
                layer::LayerStatus::NotInstalled => ("レイヤー: 未導入", egui::Color32::GRAY),
                layer::LayerStatus::Error { .. } => {
                    ("レイヤー: エラー", egui::Color32::from_rgb(240, 110, 110))
                }
            };
            ui::widgets::status_badge(ui, layer_text, layer_color);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("保存").clicked() {
                    save_clicked = true;
                }
                if self.config.dirty || self.scale.dirty {
                    ui::widgets::status_badge(
                        ui,
                        "● 未保存",
                        egui::Color32::from_rgb(230, 180, 70),
                    );
                }
                if ui.button("再読み込み").clicked() {
                    reload_clicked = true;
                }
            });
        });
        if save_clicked {
            self.save_all();
        }
        if reload_clicked {
            self.reload_all();
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("設定:");
            ui::widgets::mono_path(ui, &self.config.path);
            ui.separator();
            ui.label("スケール:");
            match &self.scale.snapshot.effective {
                Some(path) => ui::widgets::mono_path(ui, path),
                None => {
                    ui.label("なし");
                }
            }
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        for tab in [
            Tab::Upscaler,
            Tab::Scale,
            Tab::Model,
            Tab::Layer,
            Tab::Launch,
            Tab::Info,
        ] {
            if ui
                .selectable_label(self.active_tab == tab, tab.label())
                .clicked()
            {
                self.active_tab = tab;
                self.settings.active_tab = Some(tab.key().to_string());
                self.settings_dirty = true;
            }
        }
    }

    fn show_toast_area(&mut self, ui: &mut egui::Ui) {
        let expired = self
            .toast
            .as_ref()
            .is_some_and(|toast| Instant::now() >= toast.until);
        if expired {
            self.toast = None;
        }
        let Some(toast) = &self.toast else {
            return;
        };
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(200))
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui::widgets::toast(ui, &toast.text, toast.kind);
                    });
            });
    }

    fn show_confirm(&mut self, ui: &mut egui::Ui) {
        let Some(dialog) = self.confirm.as_ref() else {
            return;
        };
        let title = dialog.title.clone();
        let body = dialog.body.clone();
        let confirm_label = dialog.confirm_label.clone();
        let action = dialog.action.clone();
        let mut decision = None;
        egui::Modal::new(egui::Id::unique("confirm")).show(ui.ctx(), |ui| {
            ui.heading(title);
            ui.label(body);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(confirm_label).clicked() {
                    decision = Some(true);
                }
                if ui.button("キャンセル").clicked() {
                    decision = Some(false);
                }
            });
        });
        match decision {
            Some(true) => {
                self.confirm = None;
                self.apply_confirm(action);
            }
            Some(false) => {
                self.confirm = None;
            }
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.last_poll.elapsed() >= Duration::from_secs(1) {
            self.poll_files();
            self.last_poll = Instant::now();
        }
        self.poll_background();
        if self.model.busy.is_some() || self.layer.busy.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        let save = ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::S));
        let reload = ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F5));
        if save {
            self.save_all();
        }
        if reload {
            self.reload_all();
        }

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("nav")
            .resizable(false)
            .default_size(190.0)
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.active_tab {
                Tab::Upscaler => ui::upscaler::show(self, ui),
                Tab::Scale => ui::scale::show(self, ui),
                Tab::Model => ui::model::show(self, ui),
                Tab::Layer => ui::layer::show(self, ui),
                Tab::Launch => ui::launch::show(self, ui),
                Tab::Info => ui::info::show(self, ui),
            });
        });

        self.show_toast_area(ui);
        self.show_confirm(ui);
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::Env;

    fn temp_env() -> (tempfile::TempDir, Env) {
        let dir = tempfile::tempdir().unwrap();
        let env = Env {
            home: Some(dir.path().join("home")),
            xdg_config_home: Some(dir.path().join("config")),
            xdg_runtime_dir: Some(dir.path().join("run")),
            ..Default::default()
        };
        (dir, env)
    }

    fn config_path(env: &Env) -> PathBuf {
        env.xdg_config_home
            .as_ref()
            .unwrap()
            .join("xrizer/upscaler.json")
    }

    fn write_config(env: &Env, text: &str) {
        let path = config_path(env);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn save_preserves_unknown_keys_and_clears_dirty() {
        let (_dir, env) = temp_env();
        write_config(&env, r#"{"enabled": true, "future_key": 42}"#);
        let mut view = ConfigView::new(&env);
        assert!(matches!(view.state, ConfigState::Loaded));
        view.form.radius = 0.8;
        view.recompute_dirty();
        assert!(view.dirty);
        assert!(view.save(&env, false));
        assert!(!view.dirty);
        let text = std::fs::read_to_string(config_path(&env)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["future_key"], 42);
        assert_eq!(value["radius"].as_f64().unwrap(), f64::from(0.8_f32));
        assert_eq!(value["enabled"], true);
    }

    #[test]
    fn refuses_to_save_a_broken_file() {
        let (_dir, env) = temp_env();
        write_config(&env, "{broken");
        let mut view = ConfigView::new(&env);
        assert!(matches!(view.state, ConfigState::Broken { .. }));
        assert!(!view.save(&env, false));
        assert_eq!(
            std::fs::read_to_string(config_path(&env)).unwrap(),
            "{broken"
        );
    }

    #[test]
    fn scale_view_saves_to_the_effective_path() {
        let (_dir, env) = temp_env();
        let runtime = env.xdg_runtime_dir.as_ref().unwrap();
        std::fs::create_dir_all(runtime).unwrap();
        let path = runtime.join("xrizer_scale");
        std::fs::write(&path, "0.500").unwrap();
        let mut view = ScaleView::new(&env);
        assert_eq!(view.form_value, 0.5);
        view.form_value = 0.7;
        view.recompute_dirty();
        assert!(view.dirty);
        assert!(view.save(&env, false));
        assert!(!view.dirty);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "0.700");
        assert!(path.with_file_name("xrizer_scale.bak").exists());
    }

    #[test]
    fn scale_view_detects_conflict_and_allows_forced_save() {
        let (_dir, env) = temp_env();
        let runtime = env.xdg_runtime_dir.as_ref().unwrap();
        std::fs::create_dir_all(runtime).unwrap();
        let path = runtime.join("xrizer_scale");
        std::fs::write(&path, "0.500").unwrap();
        let mut view = ScaleView::new(&env);
        view.form_value = 0.7;
        view.recompute_dirty();
        std::fs::write(&path, "0.900").unwrap();
        assert!(!view.save(&env, false));
        assert!(view.conflict);
        assert!(view.save(&env, true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "0.700");
    }

    #[test]
    fn detects_external_change_and_allows_forced_save() {
        let (_dir, env) = temp_env();
        write_config(&env, r#"{"radius": 0.35}"#);
        let mut view = ConfigView::new(&env);
        view.form.radius = 0.8;
        view.recompute_dirty();
        std::fs::write(config_path(&env), r#"{"radius": 0.5}"#).unwrap();
        assert!(!view.save(&env, false));
        assert!(view.conflict);
        assert!(view.save(&env, true));
        let text = std::fs::read_to_string(config_path(&env)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["radius"].as_f64().unwrap(), f64::from(0.8_f32));
    }
}
