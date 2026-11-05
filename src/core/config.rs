//! `upscaler.json` loading and saving.
//!
//! The file is edited as raw JSON so unknown keys survive a save. Loading is
//! strict: malformed JSON or a known key with the wrong type is reported as
//! broken and is never rewritten automatically.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::core::atomic::{BackupPolicy, WriteOutcome, can_write_into, write_atomic};
use crate::core::env::{Env, config_candidates};

pub const KNOWN_KEYS: [&str; 14] = [
    "enabled",
    "preset",
    "center",
    "radius",
    "feather",
    "strength",
    "output_scale",
    "preserve_alpha",
    "compare",
    "show_region",
    "model_path",
    "auto_scale",
    "min_scale",
    "max_scale",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Fast,
    #[default]
    Balanced,
    Fsr,
    Ml,
}

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Balanced => "balanced",
            Self::Fsr => "fsr",
            Self::Ml => "ml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpscalerConfig {
    pub enabled: bool,
    pub preset: Preset,
    pub center: [f32; 2],
    pub radius: f32,
    pub feather: f32,
    pub strength: f32,
    pub output_scale: f32,
    pub preserve_alpha: bool,
    pub compare: bool,
    pub show_region: bool,
    pub model_path: Option<PathBuf>,
    pub auto_scale: bool,
    pub min_scale: f32,
    pub max_scale: f32,
}

impl Default for UpscalerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: Preset::Balanced,
            center: [0.5, 0.5],
            radius: 0.35,
            feather: 0.10,
            strength: 1.0,
            output_scale: 1.0,
            preserve_alpha: true,
            compare: false,
            show_region: false,
            model_path: None,
            auto_scale: false,
            min_scale: 0.5,
            max_scale: 1.5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub raw: Map<String, Value>,
    pub value: UpscalerConfig,
    pub notices: Vec<String>,
    pub existed: bool,
}

#[derive(Debug, Clone)]
pub struct BrokenConfig {
    pub path: PathBuf,
    pub error: String,
    pub raw_text: String,
}

#[derive(Debug, Clone)]
pub struct ConfigPathInfo {
    pub candidates: Vec<PathBuf>,
    pub path: PathBuf,
    pub existed: bool,
}

/// Resolve the config file the game will read: the first existing candidate,
/// even if that file is broken. A lower-priority file is never silently used.
pub fn resolve_path(env: &Env) -> Result<ConfigPathInfo, String> {
    let candidates = config_candidates(env);
    for path in &candidates {
        if path.exists() {
            return Ok(ConfigPathInfo {
                candidates: candidates.clone(),
                path: path.clone(),
                existed: true,
            });
        }
    }
    for path in &candidates {
        let parent = path.parent().unwrap_or_else(|| Path::new("/"));
        if can_write_into(parent) {
            return Ok(ConfigPathInfo {
                candidates: candidates.clone(),
                path: path.clone(),
                existed: false,
            });
        }
    }
    Err(format!(
        "設定ファイルの書き込み可能な候補がありません: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Load a config file. A missing file is a valid empty config; every other
/// failure is a `BrokenConfig` so the caller can refuse to save.
pub fn load(path: &Path) -> Result<LoadedConfig, BrokenConfig> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedConfig {
                path: path.to_path_buf(),
                raw: Map::new(),
                value: UpscalerConfig::default(),
                notices: Vec::new(),
                existed: false,
            });
        }
        Err(error) => {
            return Err(BrokenConfig {
                path: path.to_path_buf(),
                error: format!("設定ファイルを読めません: {error}"),
                raw_text: String::new(),
            });
        }
    };
    let text = String::from_utf8(bytes).map_err(|error| BrokenConfig {
        path: path.to_path_buf(),
        error: format!("設定ファイルが UTF-8 ではありません: {error}"),
        raw_text: String::new(),
    })?;
    let value: Value = serde_json::from_str(&text).map_err(|error| BrokenConfig {
        path: path.to_path_buf(),
        error: format!("設定ファイルの JSON が不正です: {error}"),
        raw_text: text.clone(),
    })?;
    let Some(object) = value.as_object() else {
        return Err(BrokenConfig {
            path: path.to_path_buf(),
            error: "設定ファイルのトップレベルがオブジェクトではありません".to_string(),
            raw_text: text,
        });
    };
    let raw = object.clone();
    let mut config: UpscalerConfig =
        serde_json::from_value(value).map_err(|error| BrokenConfig {
            path: path.to_path_buf(),
            error: format!("設定値の型が不正です: {error}"),
            raw_text: text.clone(),
        })?;
    let (sanitized, mut notices) = sanitize(config.clone());
    config = sanitized;
    if config
        .model_path
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        config.model_path = None;
        notices.push("空の model_path を null として扱います".to_string());
    } else if config
        .model_path
        .as_ref()
        .is_some_and(|path| path.is_relative())
    {
        notices.push(
            "model_path が相対パスです。ゲームの作業ディレクトリ基準になるため、絶対パスにしてください"
                .to_string(),
        );
    }
    Ok(LoadedConfig {
        path: path.to_path_buf(),
        raw,
        value: config,
        notices,
        existed: true,
    })
}

/// Clamp every numeric field into the documented range and normalize the
/// min/max scale pair. Each actual change produces one Japanese notice.
pub fn sanitize(value: UpscalerConfig) -> (UpscalerConfig, Vec<String>) {
    let defaults = UpscalerConfig::default();
    let mut notices = Vec::new();
    let mut value = value;

    value.center = [
        finite_clamped(
            value.center[0],
            defaults.center[0],
            0.0,
            1.0,
            "center.x",
            &mut notices,
        ),
        finite_clamped(
            value.center[1],
            defaults.center[1],
            0.0,
            1.0,
            "center.y",
            &mut notices,
        ),
    ];
    value.radius = finite_clamped(
        value.radius,
        defaults.radius,
        0.05,
        2.0,
        "radius",
        &mut notices,
    );
    value.feather = finite_clamped(
        value.feather,
        defaults.feather,
        0.0,
        1.0,
        "feather",
        &mut notices,
    );
    value.strength = finite_clamped(
        value.strength,
        defaults.strength,
        0.0,
        3.0,
        "strength",
        &mut notices,
    );
    value.output_scale = finite_clamped(
        value.output_scale,
        defaults.output_scale,
        0.5,
        2.0,
        "output_scale",
        &mut notices,
    );
    value.min_scale = finite_clamped(
        value.min_scale,
        defaults.min_scale,
        0.1,
        4.0,
        "min_scale",
        &mut notices,
    );
    value.max_scale = finite_clamped(
        value.max_scale,
        defaults.max_scale,
        0.1,
        4.0,
        "max_scale",
        &mut notices,
    );
    if value.min_scale > value.max_scale {
        std::mem::swap(&mut value.min_scale, &mut value.max_scale);
        notices.push("min_scale と max_scale を入れ替えました".to_string());
    }
    (value, notices)
}

fn finite_clamped(
    raw: f32,
    default: f32,
    min: f32,
    max: f32,
    name: &str,
    notices: &mut Vec<String>,
) -> f32 {
    let finite = if raw.is_finite() {
        raw
    } else {
        notices.push(format!(
            "{name} が有限値ではないため既定値 {default} を使用します"
        ));
        default
    };
    let clamped = finite.clamp(min, max);
    if (clamped - finite).abs() > f32::EPSILON {
        notices.push(format!(
            "{name} を {clamped} に丸めました（範囲 {min}〜{max}）"
        ));
    }
    clamped
}

/// Normalize the GUI's `model_path` text. Empty text is `None`, `~` is
/// expanded, and relative paths are rejected because the game resolves them
/// against its own working directory.
pub fn normalize_model_path(
    raw: Option<&str>,
    home: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let path = if trimmed == "~" {
        home.ok_or_else(|| "HOME が未設定のため ~ を展開できません".to_string())?
            .to_path_buf()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home.ok_or_else(|| "HOME が未設定のため ~ を展開できません".to_string())?
            .join(rest)
    } else if trimmed.starts_with('~') {
        return Err(
            "~user 形式のパスは未対応です。$HOME からの相対パスを使ってください".to_string(),
        );
    } else {
        PathBuf::from(trimmed)
    };
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        Err("model_path は絶対パスで指定してください".to_string())
    }
}

/// Build the JSON that will be written: unknown keys and their order are kept,
/// while all known keys are replaced with the current form values.
pub fn build_json(raw: &Map<String, Value>, value: &UpscalerConfig) -> Result<String, String> {
    let mut map = raw.clone();
    let serialized = serde_json::to_value(value)
        .map_err(|error| format!("設定を JSON に変換できません: {error}"))?;
    let object = serialized
        .as_object()
        .ok_or_else(|| "設定の JSON 変換結果がオブジェクトではありません".to_string())?;
    for (key, item) in object {
        map.insert(key.clone(), item.clone());
    }
    let mut text = serde_json::to_string_pretty(&map)
        .map_err(|error| format!("設定を整形できません: {error}"))?;
    text.push('\n');
    Ok(text)
}

/// Save after verifying that the generated JSON can be parsed back.
pub fn save(
    path: &Path,
    raw: &Map<String, Value>,
    value: &UpscalerConfig,
) -> Result<WriteOutcome, String> {
    let text = build_json(raw, value)?;
    serde_json::from_str::<UpscalerConfig>(&text)
        .map_err(|error| format!("生成した設定 JSON を検証できません: {error}"))?;
    write_atomic(path, text.as_bytes(), BackupPolicy::BeforeReplace)
}

/// Content fingerprint used for external-change detection.
pub fn file_fingerprint(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::Env;

    fn valid_json() -> &'static str {
        r#"{
      "enabled": true,
      "preset": "ml",
      "center": [0.5, 0.5],
      "radius": 0.35,
      "feather": 0.1,
      "strength": 1.0,
      "output_scale": 1.0,
      "preserve_alpha": true,
      "compare": false,
      "show_region": false,
      "model_path": null,
      "auto_scale": false,
      "min_scale": 0.5,
      "max_scale": 1.5,
      "future_key": {"nested": [1, 2, 3]},
      "another_key": "keep me"
    }"#
    }

    #[test]
    fn preserves_unknown_keys_and_order_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upscaler.json");
        std::fs::write(&path, valid_json()).unwrap();
        let mut loaded = load(&path).unwrap();
        loaded.value.radius = 0.8;
        let text = build_json(&loaded.raw, &loaded.value).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["future_key"]["nested"][2], 3);
        assert_eq!(value["another_key"], "keep me");
        assert_eq!(value["radius"].as_f64().unwrap(), f64::from(0.8_f32));
        let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys[0], "enabled");
        assert_eq!(keys[keys.len() - 1], "another_key");
        serde_json::from_str::<UpscalerConfig>(&text).unwrap();
    }

    #[test]
    fn broken_json_is_reported_not_repaired() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upscaler.json");
        std::fs::write(&path, r#"{"enabled": true, "#).unwrap();
        let broken = load(&path).unwrap_err();
        assert!(!broken.error.is_empty());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"enabled": true, "#
        );
    }

    #[test]
    fn wrong_known_key_type_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upscaler.json");
        std::fs::write(&path, r#"{"enabled": "yes"}"#).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn sanitizes_ranges_and_swaps_scale_bounds() {
        let value = UpscalerConfig {
            radius: 9.0,
            feather: -1.0,
            center: [-2.0, 3.0],
            min_scale: 2.0,
            max_scale: 0.5,
            ..Default::default()
        };
        let (value, notices) = sanitize(value);
        assert_eq!(value.radius, 2.0);
        assert_eq!(value.feather, 0.0);
        assert_eq!(value.center, [0.0, 1.0]);
        assert_eq!(value.min_scale, 0.5);
        assert_eq!(value.max_scale, 2.0);
        assert_eq!(notices.len(), 5);
    }

    #[test]
    fn resolves_first_existing_even_if_it_is_broken() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a/upscaler.json");
        let second = dir.path().join("b/xrizer/upscaler.json");
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::write(&first, "{broken").unwrap();
        std::fs::write(&second, "{}").unwrap();
        let env = Env {
            xrizer_upscaler_config: Some(first.clone()),
            xdg_config_home: Some(dir.path().join("b")),
            ..Default::default()
        };
        let info = resolve_path(&env).unwrap();
        assert_eq!(info.path, first);
        assert!(info.existed);
    }

    #[test]
    fn missing_config_uses_first_writable_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env {
            home: Some(dir.path().into()),
            ..Default::default()
        };
        let info = resolve_path(&env).unwrap();
        assert_eq!(info.path, dir.path().join(".config/xrizer/upscaler.json"));
        assert!(!info.existed);
    }

    #[test]
    fn model_path_expands_tilde_and_rejects_relative() {
        let home = std::path::Path::new("/home/u");
        assert_eq!(
            normalize_model_path(Some("~/m.xru"), Some(home)).unwrap(),
            Some(PathBuf::from("/home/u/m.xru"))
        );
        assert_eq!(normalize_model_path(Some("   "), Some(home)).unwrap(), None);
        assert!(normalize_model_path(Some("models/m.xru"), Some(home)).is_err());
        assert!(normalize_model_path(Some("~other/m.xru"), Some(home)).is_err());
    }

    #[test]
    fn save_writes_parent_backup_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("xrizer/upscaler.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        let loaded = load(&path).unwrap();
        let mut value = loaded.value.clone();
        value.enabled = true;
        let outcome = save(&path, &loaded.raw, &value).unwrap();
        assert!(outcome.backup.unwrap().exists());
        assert!(load(&path).unwrap().value.enabled);
    }
}
