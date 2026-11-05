//! Application state and top-level eframe shell.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::core::config::{self, UpscalerConfig};
use crate::core::env::{self, Env};
use crate::core::launch::LaunchOutput;
use crate::core::layer::{self, InstallResult, LayerSnapshot, SoInfo};
use crate::core::model::{self, ModelInfo, ModelInstallOutcome};
use crate::core::scale::{self, EffectiveSource, ScaleSnapshot};
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
    ReloadScale,
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
        let (path, candidates, resolved_existed) = match config::resolve_path(env) {
            Ok(info) => (info.path, info.candidates, info.existed),
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
            state: if resolved_existed {
                ConfigState::Loaded
            } else {
                ConfigState::Missing
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
                self.save_message = None;
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
                self.save_message = None;
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
        let (value, _) = config::sanitize(value);
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
        self.save_message = None;
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
                let backup = outcome
                    .backup
                    .as_ref()
                    .map(|path| format!(" / バックアップ: {}", path.display()))
                    .unwrap_or_default();
                let previous = outcome
                    .previous_value
                    .map_or_else(|| "新規".to_string(), |value| format!("{value:.3}"));
                self.save_message = Some(format!(
                    "保存しました: {}（以前: {previous}）{backup}",
                    outcome.path.display()
                ));
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
    pub installed_hash: Option<u64>,
    pub installed_hash_for: Option<(PathBuf, u64, Option<std::time::SystemTime>)>,
    pub so_key: Option<(u64, Option<std::time::SystemTime>)>,
}

impl LayerView {
    pub fn new(env: &Env, settings: &AppSettings) -> Self {
        let so_text = settings
            .layer_so_path
            .clone()
            .or_else(|| layer::auto_detect_so(env).map(|path| path.display().to_string()))
            .unwrap_or_default();
        let mut view = Self {
            so_text,
            so: None,
            snapshot: layer::snapshot(env),
            busy: None,
            log: String::new(),
            error: None,
            installed_hash: None,
            installed_hash_for: None,
            so_key: None,
        };
        view.refresh_so();
        view.update_installed_hash();
        view
    }

    pub fn refresh(&mut self, env: &Env) {
        self.snapshot = layer::snapshot(env);
        self.update_installed_hash();
    }

    pub fn update_installed_hash(&mut self) {
        let key = match &self.snapshot.status {
            layer::LayerStatus::Installed {
                library_path,
                lib_size,
            } => {
                let modified = std::fs::metadata(library_path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok());
                Some((library_path.clone(), *lib_size, modified))
            }
            _ => None,
        };
        match key {
            Some(key) => {
                if self.installed_hash_for.as_ref() != Some(&key) {
                    self.installed_hash = layer::hash_file(&key.0).ok();
                    self.installed_hash_for = Some(key);
                }
            }
            None => {
                self.installed_hash = None;
                self.installed_hash_for = None;
            }
        }
    }

    pub fn refresh_so(&mut self) {
        let text = self.so_text.trim();
        if text.is_empty() {
            self.so = None;
            self.so_key = None;
            return;
        }
        let path = Path::new(text);
        self.so_key = std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        self.so = Some(layer::inspect_so(path));
    }

    pub fn refresh_so_if_changed(&mut self) {
        let text = self.so_text.trim();
        if text.is_empty() {
            self.so = None;
            self.so_key = None;
            return;
        }
        let path = Path::new(text);
        let key = std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()));
        if key != self.so_key {
            self.refresh_so();
        }
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
        Self::from_env(Env::from_process())
    }

    pub(crate) fn from_env(env: Env) -> Self {
        let loaded = settings::load(&env);
        let mut settings = loaded.settings;
        let settings_broken = loaded.broken;
        let mut settings_warning = loaded.warning;
        let startup_warning = settings_warning.clone();
        let mut settings_dirty = false;

        if settings.launch.capture_dir.is_empty()
            && let Some(home) = &env.home
        {
            settings.launch.capture_dir = home.join("xrizer-captures").display().to_string();
            settings_dirty = !settings_broken;
        }
        let notices = settings.launch.sanitize();
        if !notices.is_empty() {
            if !settings_broken {
                settings_dirty = true;
            }
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

        let mut app = Self {
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
        };
        if let Some(warning) = startup_warning {
            app.toast = Some(Toast {
                text: format!(
                    "GUI 設定を読めませんでした。既定値で起動し、保存時に settings.json.bak を作成します: {warning}"
                ),
                kind: ToastKind::Error,
                until: Instant::now() + Duration::from_secs(10),
            });
        }
        app
    }

    pub fn start_layer_install(&mut self, so_path: PathBuf) {
        let env = self.env.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = layer::install(&env, &so_path);
            let _ = sender.send(result);
        });
        self.layer.busy = Some(LayerBusy::Install(receiver));
        self.layer.error = None;
    }

    pub fn start_layer_remove(&mut self) {
        let env = self.env.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = layer::remove(&env);
            let _ = sender.send(result);
        });
        self.layer.busy = Some(LayerBusy::Remove(receiver));
        self.layer.error = None;
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
        let config_broken = matches!(&self.config.state, ConfigState::Broken { .. });
        let config_needs = !config_broken
            && (self.config.dirty
                || matches!(&self.config.state, ConfigState::Missing)
                || !self.config.notices.is_empty());
        let scale_needs = self.scale.dirty
            || matches!(&self.scale.snapshot.source, Some(EffectiveSource::NewFile));

        let mut summary = Vec::new();
        let mut had_error = false;
        let mut saved_any = false;

        if config_broken {
            summary.push("設定: エラー（読込エラー）".to_string());
            had_error = true;
        } else if config_needs {
            if self.config.save(&self.env, false) {
                summary.push("設定: 保存".to_string());
                saved_any = true;
            } else {
                let error = self
                    .config
                    .error
                    .clone()
                    .unwrap_or_else(|| "保存できませんでした".to_string());
                summary.push(format!("設定: エラー（{error}）"));
                had_error = true;
                if self.config.conflict {
                    self.confirm = Some(ConfirmDialog {
                        title: "設定ファイルを上書き".to_string(),
                        body: "外部で変更された設定ファイルを、現在の内容で上書きしますか？"
                            .to_string(),
                        confirm_label: "上書き保存".to_string(),
                        action: ConfirmAction::OverwriteConfig,
                    });
                }
            }
        } else {
            summary.push("設定: 変更なし".to_string());
        }

        if scale_needs {
            if self.scale.save(&self.env, false) {
                summary.push("スケール: 保存".to_string());
                saved_any = true;
            } else {
                let error = self
                    .scale
                    .error
                    .clone()
                    .unwrap_or_else(|| "保存できませんでした".to_string());
                summary.push(format!("スケール: エラー（{error}）"));
                had_error = true;
                if self.scale.conflict {
                    self.confirm = Some(ConfirmDialog {
                        title: "スケールファイルを上書き".to_string(),
                        body: "外部で変更されたスケールファイルを、現在の値で上書きしますか？"
                            .to_string(),
                        confirm_label: "上書き保存".to_string(),
                        action: ConfirmAction::OverwriteScale,
                    });
                }
            }
        } else {
            summary.push("スケール: 変更なし".to_string());
        }

        let text = summary.join(" / ");
        if had_error {
            self.show_toast(ToastKind::Error, text);
        } else if saved_any {
            self.show_toast(ToastKind::Success, text);
        } else {
            self.show_toast(ToastKind::Info, "変更はありません");
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

    pub fn poll_files(&mut self) {
        let config_fingerprint = config::file_fingerprint(&self.config.path);
        if config_fingerprint != self.config.fingerprint {
            if self.config.dirty {
                self.config.conflict = true;
            } else {
                self.config.reload(&self.env);
                self.show_toast(ToastKind::Info, "設定ファイルの外部変更を読み込みました");
            }
        }

        let scale_snapshot = scale::snapshot(&self.env);
        if scale_snapshot.fingerprint != self.scale.loaded_fingerprint {
            if self.scale.dirty {
                self.scale.conflict = true;
            } else {
                self.scale.snapshot = scale_snapshot;
                self.scale.form_value = self.scale.snapshot.effective_value.unwrap_or(1.0);
                self.scale.saved_value = self.scale.form_value;
                self.scale.loaded_fingerprint = self.scale.snapshot.fingerprint;
                self.scale.error = None;
                self.scale.conflict = false;
                self.show_toast(
                    ToastKind::Info,
                    "スケールファイルの外部変更を読み込みました",
                );
            }
        }

        self.layer.refresh(&self.env);
        self.layer.refresh_so_if_changed();
        let target = self.model.target.clone();
        self.model.refresh_current(&target);
    }

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

        let layer_result = match &self.layer.busy {
            Some(LayerBusy::Install(receiver)) => Some((true, receiver.try_recv())),
            Some(LayerBusy::Remove(receiver)) => Some((false, receiver.try_recv())),
            None => None,
        };
        match layer_result {
            Some((install, Ok(result))) => {
                self.layer.busy = None;
                match result {
                    Ok(result) => {
                        self.layer.snapshot = result.snapshot;
                        self.layer.log = format!("{}\n{}", result.stdout, result.stderr)
                            .trim()
                            .to_string();
                        self.layer.refresh_so();
                        self.layer.update_installed_hash();
                        self.show_toast(
                            ToastKind::Success,
                            if install {
                                "Vulkan レイヤーをインストールしました"
                            } else {
                                "Vulkan レイヤーを削除しました"
                            },
                        );
                    }
                    Err(error) => {
                        self.layer.error = Some(error.clone());
                        self.show_toast(ToastKind::Error, error);
                    }
                }
            }
            Some((_, Err(std::sync::mpsc::TryRecvError::Empty))) => {}
            Some((_, Err(std::sync::mpsc::TryRecvError::Disconnected))) => {
                self.layer.busy = None;
                self.layer.error = Some("レイヤー処理スレッドが終了しました".to_string());
            }
            None => {}
        }

        if self.settings_dirty && self.last_settings_save.elapsed() >= Duration::from_secs(1) {
            match settings::save(&self.env, &self.settings) {
                Ok(_) => {
                    self.settings_dirty = false;
                    self.last_settings_save = Instant::now();
                }
                Err(error) => {
                    self.settings_warning = Some(format!("GUI 設定を保存できません: {error}"));
                    self.last_settings_save = Instant::now();
                }
            }
        }
    }

    pub fn apply_confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::ReloadAll => self.reload_all_now(),
            ConfirmAction::ReloadScale => self.scale.reload(&self.env),
            ConfirmAction::InstallModel { source, target } => {
                self.start_model_install(source, target);
            }
            ConfirmAction::RemoveLayer => self.start_layer_remove(),
            ConfirmAction::OverwriteConfig => {
                if self.config.save(&self.env, true) {
                    let message = self
                        .config
                        .save_message
                        .clone()
                        .unwrap_or_else(|| "設定を保存しました".to_string());
                    self.show_toast(ToastKind::Success, message);
                } else {
                    let error = self
                        .config
                        .error
                        .clone()
                        .unwrap_or_else(|| "設定を保存できませんでした".to_string());
                    self.show_toast(ToastKind::Error, error);
                }
            }
            ConfirmAction::OverwriteScale => {
                if self.scale.save(&self.env, true) {
                    let message = self
                        .scale
                        .save_message
                        .clone()
                        .unwrap_or_else(|| "スケールを保存しました".to_string());
                    self.show_toast(ToastKind::Success, message);
                } else {
                    let error = self
                        .scale
                        .error
                        .clone()
                        .unwrap_or_else(|| "スケールを保存できませんでした".to_string());
                    self.show_toast(ToastKind::Error, error);
                }
            }
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

    fn on_exit(&mut self, _gl: Option<&glow::Context>) {
        if self.settings_dirty {
            let _ = settings::save(&self.env, &self.settings);
        }
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
    fn broken_gui_settings_are_not_auto_saved_at_startup() {
        let (_dir, env) = temp_env();
        let settings_path = crate::core::settings::path(&env);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(&settings_path, "{broken").unwrap();
        let app = App::from_env(env.clone());
        assert!(!app.settings_dirty);
        assert!(app.settings_warning.is_some());
        assert_eq!(std::fs::read_to_string(&settings_path).unwrap(), "{broken");
    }

    #[test]
    fn first_save_creates_missing_config_and_scale() {
        let (_dir, env) = temp_env();
        let mut app = App::from_env(env.clone());
        assert!(matches!(app.config.state, ConfigState::Missing));
        assert!(matches!(
            app.scale.snapshot.source,
            Some(EffectiveSource::NewFile)
        ));
        app.save_all();
        assert!(config_path(&env).exists());
        let scale_path = env.home.as_ref().unwrap().join(".config/xrizer_scale");
        assert!(scale_path.exists());
        assert_eq!(std::fs::read_to_string(scale_path).unwrap(), "1.000");
        assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Success);
    }

    #[test]
    fn broken_config_does_not_hide_successful_scale_save() {
        let (_dir, env) = temp_env();
        write_config(&env, "{broken");
        let runtime = env.xdg_runtime_dir.as_ref().unwrap();
        std::fs::create_dir_all(runtime).unwrap();
        let scale_path = runtime.join("xrizer_scale");
        std::fs::write(&scale_path, "0.500").unwrap();
        let mut app = App::from_env(env.clone());
        assert!(matches!(app.config.state, ConfigState::Broken { .. }));
        app.scale.form_value = 0.7;
        app.scale.recompute_dirty();
        app.save_all();
        assert_eq!(std::fs::read_to_string(&scale_path).unwrap(), "0.700");
        let toast = app.toast.as_ref().unwrap();
        assert_eq!(toast.kind, ToastKind::Error);
        assert!(toast.text.contains("スケール: 保存"));
        assert!(toast.text.contains("設定: エラー"));
    }

    #[test]
    fn save_normalizes_reversed_scale_bounds() {
        let (_dir, env) = temp_env();
        write_config(&env, r#"{"radius": 0.35}"#);
        let mut view = ConfigView::new(&env);
        view.form.min_scale = 3.0;
        view.form.max_scale = 0.5;
        view.recompute_dirty();
        assert!(view.save(&env, false));
        let text = std::fs::read_to_string(config_path(&env)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["min_scale"].as_f64().unwrap(), 0.5);
        assert_eq!(value["max_scale"].as_f64().unwrap(), 3.0);
    }

    #[test]
    fn poll_files_reloads_config_when_clean_and_flags_conflict_when_dirty() {
        let (_dir, env) = temp_env();
        write_config(&env, r#"{"radius": 0.35}"#);
        let mut app = App::from_env(env.clone());
        assert!(matches!(app.config.state, ConfigState::Loaded));
        std::fs::write(config_path(&env), r#"{"radius": 0.5}"#).unwrap();
        app.poll_files();
        assert_eq!(app.config.form.radius, 0.5);
        app.config.form.radius = 0.8;
        app.config.recompute_dirty();
        std::fs::write(config_path(&env), r#"{"radius": 0.6}"#).unwrap();
        app.poll_files();
        assert!(app.config.conflict);
        assert_eq!(app.config.form.radius, 0.8);
    }

    #[test]
    fn poll_files_reloads_scale_when_clean_and_flags_conflict_when_dirty() {
        let (_dir, env) = temp_env();
        let runtime = env.xdg_runtime_dir.as_ref().unwrap();
        std::fs::create_dir_all(runtime).unwrap();
        let path = runtime.join("xrizer_scale");
        std::fs::write(&path, "0.500").unwrap();
        let mut app = App::from_env(env.clone());
        assert_eq!(app.scale.form_value, 0.5);
        std::fs::write(&path, "0.700").unwrap();
        app.poll_files();
        assert_eq!(app.scale.form_value, 0.7);
        app.scale.form_value = 0.9;
        app.scale.recompute_dirty();
        std::fs::write(&path, "0.800").unwrap();
        app.poll_files();
        assert!(app.scale.conflict);
        assert_eq!(app.scale.form_value, 0.9);
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
    fn layer_view_refreshes_selected_so_when_file_changes() {
        let (_dir, env) = temp_env();
        let home = env.home.as_ref().unwrap();
        std::fs::create_dir_all(home).unwrap();
        let so = home.join("libxrizer_upscaler_layer.so");
        std::fs::write(&so, b"\x7fELF\x01\x01\x01\x00dummy").unwrap();
        let settings = AppSettings {
            layer_so_path: Some(so.display().to_string()),
            ..Default::default()
        };
        let mut view = LayerView::new(&env, &settings);
        let first = view.so.as_ref().unwrap().as_ref().unwrap().hash;
        std::fs::write(&so, b"\x7fELF\x01\x01\x01\x00different").unwrap();
        view.refresh_so_if_changed();
        let second = view.so.as_ref().unwrap().as_ref().unwrap().hash;
        assert_ne!(first, second);
    }

    #[test]
    fn layer_view_caches_installed_hash() {
        let (_dir, env) = temp_env();
        let home = env.home.as_ref().unwrap();
        std::fs::create_dir_all(home).unwrap();
        let so = home.join("libxrizer_upscaler_layer.so");
        std::fs::write(&so, b"\x7fELF\x01\x01\x01\x00dummy").unwrap();
        let installed = layer::install(&env, &so).unwrap();
        assert!(matches!(
            installed.snapshot.status,
            layer::LayerStatus::Installed { .. }
        ));
        let view = LayerView::new(&env, &AppSettings::default());
        assert_eq!(view.installed_hash, layer::hash_file(&so).ok());
    }

    #[test]
    fn reload_scale_confirmation_discards_edits() {
        let (_dir, env) = temp_env();
        let runtime = env.xdg_runtime_dir.as_ref().unwrap();
        std::fs::create_dir_all(runtime).unwrap();
        std::fs::write(runtime.join("xrizer_scale"), "0.500").unwrap();
        let mut app = App::from_env(env.clone());
        app.scale.form_value = 0.9;
        app.scale.recompute_dirty();
        app.apply_confirm(ConfirmAction::ReloadScale);
        assert_eq!(app.scale.form_value, 0.5);
        assert!(!app.scale.dirty);
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
