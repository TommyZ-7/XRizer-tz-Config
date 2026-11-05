//! GUI preference persistence.
//!
//! These settings are separate from `upscaler.json`; they only remember the
//! selected layer `.so`, the last model directory, and the launch-option form.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::atomic::{BackupPolicy, WriteOutcome, write_atomic};
use crate::core::env::Env;
use crate::core::launch::LaunchForm;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub layer_so_path: Option<String>,
    pub last_model_dir: Option<String>,
    pub launch: LaunchForm,
    pub active_tab: Option<String>,
}

pub fn path(env: &Env) -> PathBuf {
    if let Some(dir) = &env.xdg_config_home {
        return dir.join("xrizer-tz-config/settings.json");
    }
    if let Some(home) = &env.home {
        return home.join(".config/xrizer-tz-config/settings.json");
    }
    PathBuf::from("/tmp/xrizer-tz-config/settings.json")
}

pub fn load(env: &Env) -> (AppSettings, Option<String>) {
    let path = path(env);
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
            Ok(settings) => (settings, None),
            Err(error) => (
                AppSettings::default(),
                Some(format!(
                    "GUI 設定 {} を読めないため既定値を使います: {error}",
                    path.display()
                )),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (AppSettings::default(), None)
        }
        Err(error) => (
            AppSettings::default(),
            Some(format!(
                "GUI 設定 {} を読めないため既定値を使います: {error}",
                path.display()
            )),
        ),
    }
}

pub fn save(env: &Env, settings: &AppSettings) -> Result<WriteOutcome, String> {
    let path = path(env);
    let mut text = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("GUI 設定を JSON に変換できません: {error}"))?;
    text.push('\n');
    write_atomic(&path, text.as_bytes(), BackupPolicy::BeforeReplace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::Env;

    #[test]
    fn settings_path_follows_xdg() {
        let env = Env {
            home: Some("/home/u".into()),
            xdg_config_home: Some("/home/u/.config".into()),
            ..Default::default()
        };
        assert_eq!(
            path(&env),
            std::path::PathBuf::from("/home/u/.config/xrizer-tz-config/settings.json")
        );
    }

    #[test]
    fn roundtrips_settings() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env {
            home: Some(dir.path().into()),
            ..Default::default()
        };
        let settings = AppSettings {
            layer_so_path: Some("/tmp/lib.so".into()),
            last_model_dir: Some("/tmp/models".into()),
            launch: crate::core::launch::LaunchForm {
                capture_enabled: true,
                capture_dir: "/tmp/captures".into(),
                ..Default::default()
            },
            active_tab: Some("scale".into()),
        };
        save(&env, &settings).unwrap();
        let (loaded, warning) = load(&env);
        assert!(warning.is_none());
        assert_eq!(loaded, settings);
    }

    #[test]
    fn broken_settings_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env {
            home: Some(dir.path().into()),
            ..Default::default()
        };
        let p = path(&env);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{not json").unwrap();
        let (loaded, warning) = load(&env);
        assert!(warning.is_some());
        assert_eq!(loaded, AppSettings::default());
    }
}
