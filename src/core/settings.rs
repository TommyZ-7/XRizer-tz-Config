//! GUI preference persistence.
//!
//! These settings are separate from `upscaler.json`; they only remember the
//! selected layer `.so`, the last model directory, and the launch-option form.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::atomic::{WriteOutcome, write_atomic};
use crate::core::env::Env;
use crate::core::launch::LaunchForm;

const SETTINGS_DIR: &str = "tz-controller";
const LEGACY_SETTINGS_DIR: &str = "xrizer-tz-config";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub layer_so_path: Option<String>,
    pub last_model_dir: Option<String>,
    pub launch: LaunchForm,
    pub active_tab: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsLoad {
    pub settings: AppSettings,
    pub warning: Option<String>,
    pub broken: bool,
}

pub fn path(env: &Env) -> PathBuf {
    path_in(env, SETTINGS_DIR)
}

fn legacy_path(env: &Env) -> PathBuf {
    path_in(env, LEGACY_SETTINGS_DIR)
}

fn path_in(env: &Env, dir_name: &str) -> PathBuf {
    if let Some(dir) = &env.xdg_config_home {
        return dir.join(dir_name).join("settings.json");
    }
    if let Some(home) = &env.home {
        return home.join(".config").join(dir_name).join("settings.json");
    }
    PathBuf::from("/tmp").join(dir_name).join("settings.json")
}

enum ReadResult {
    Settings(AppSettings),
    Missing,
    Broken(String),
}

fn read(path: &Path) -> ReadResult {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
            Ok(settings) => ReadResult::Settings(settings),
            Err(error) => ReadResult::Broken(format!(
                "GUI 設定 {} を読めないため既定値を使います: {error}",
                path.display()
            )),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ReadResult::Missing,
        Err(error) => ReadResult::Broken(format!(
            "GUI 設定 {} を読めないため既定値を使います: {error}",
            path.display()
        )),
    }
}

pub fn load(env: &Env) -> SettingsLoad {
    let path = path(env);
    match read(&path) {
        ReadResult::Settings(settings) => SettingsLoad {
            settings,
            warning: None,
            broken: false,
        },
        ReadResult::Broken(warning) => SettingsLoad {
            settings: AppSettings::default(),
            warning: Some(warning),
            broken: true,
        },
        ReadResult::Missing => match read(&legacy_path(env)) {
            ReadResult::Settings(settings) => SettingsLoad {
                settings,
                warning: None,
                broken: false,
            },
            ReadResult::Missing => SettingsLoad {
                settings: AppSettings::default(),
                warning: None,
                broken: false,
            },
            ReadResult::Broken(warning) => SettingsLoad {
                settings: AppSettings::default(),
                warning: Some(warning),
                broken: true,
            },
        },
    }
}

pub fn save(env: &Env, settings: &AppSettings) -> Result<WriteOutcome, String> {
    let path = path(env);
    let mut text = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("GUI 設定を JSON に変換できません: {error}"))?;
    text.push('\n');
    write_atomic(&path, text.as_bytes())
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
            ..Default::default()
        };
        (dir, env)
    }

    fn write_settings(path: &Path, settings: &AppSettings) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_string_pretty(settings).unwrap()).unwrap();
    }

    #[test]
    fn settings_path_follows_xdg() {
        let env = Env {
            home: Some("/home/u".into()),
            xdg_config_home: Some("/home/u/.config".into()),
            ..Default::default()
        };
        assert_eq!(
            path(&env),
            std::path::PathBuf::from("/home/u/.config/tz-controller/settings.json")
        );
        assert_eq!(
            legacy_path(&env),
            std::path::PathBuf::from("/home/u/.config/xrizer-tz-config/settings.json")
        );
    }

    #[test]
    fn settings_path_falls_back_to_home_and_tmp() {
        let home_only = Env {
            home: Some("/home/u".into()),
            ..Default::default()
        };
        assert_eq!(
            path(&home_only),
            std::path::PathBuf::from("/home/u/.config/tz-controller/settings.json")
        );
        assert_eq!(
            path(&Env::default()),
            std::path::PathBuf::from("/tmp/tz-controller/settings.json")
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
        let loaded = load(&env);
        assert!(loaded.warning.is_none());
        assert!(!loaded.broken);
        assert_eq!(loaded.settings, settings);
    }

    #[test]
    fn loads_legacy_settings_when_new_path_is_missing() {
        let (_dir, env) = temp_env();
        let legacy = legacy_path(&env);
        let settings = AppSettings {
            layer_so_path: Some("/tmp/legacy.so".into()),
            active_tab: Some("layer".into()),
            ..Default::default()
        };
        write_settings(&legacy, &settings);

        let loaded = load(&env);

        assert_eq!(loaded.settings, settings);
        assert!(loaded.warning.is_none());
        assert!(!loaded.broken);
        assert!(!path(&env).exists());
    }

    #[test]
    fn saving_legacy_settings_writes_the_new_path_and_keeps_the_old_file() {
        let (_dir, env) = temp_env();
        let legacy = legacy_path(&env);
        let settings = AppSettings {
            active_tab: Some("launch".into()),
            ..Default::default()
        };
        write_settings(&legacy, &settings);
        let loaded = load(&env);

        save(&env, &loaded.settings).unwrap();

        assert!(path(&env).exists());
        assert!(legacy.exists());
        assert_eq!(load(&env).settings, settings);
    }

    #[test]
    fn new_settings_win_over_legacy_settings() {
        let (_dir, env) = temp_env();
        write_settings(
            &legacy_path(&env),
            &AppSettings {
                active_tab: Some("legacy".into()),
                ..Default::default()
            },
        );
        let new = AppSettings {
            active_tab: Some("model".into()),
            ..Default::default()
        };
        write_settings(&path(&env), &new);

        let loaded = load(&env);

        assert_eq!(loaded.settings, new);
        assert!(loaded.warning.is_none());
        assert!(!loaded.broken);
    }

    #[test]
    fn broken_new_settings_do_not_fall_back_to_legacy() {
        let (_dir, env) = temp_env();
        write_settings(
            &legacy_path(&env),
            &AppSettings {
                active_tab: Some("legacy".into()),
                ..Default::default()
            },
        );
        let new_path = path(&env);
        std::fs::create_dir_all(new_path.parent().unwrap()).unwrap();
        std::fs::write(&new_path, "{not json").unwrap();

        let loaded = load(&env);

        assert_eq!(loaded.settings, AppSettings::default());
        assert!(loaded.warning.is_some());
        assert!(loaded.broken);
    }

    #[test]
    fn broken_legacy_settings_fall_back_to_defaults() {
        let (_dir, env) = temp_env();
        let legacy = legacy_path(&env);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "{not json").unwrap();

        let loaded = load(&env);

        assert_eq!(loaded.settings, AppSettings::default());
        assert!(loaded.warning.is_some());
        assert!(loaded.broken);
    }

    #[test]
    fn saving_over_a_broken_settings_file_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env {
            home: Some(dir.path().into()),
            ..Default::default()
        };
        let p = path(&env);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{not json").unwrap();
        let loaded = load(&env);
        assert!(loaded.warning.is_some());
        assert!(loaded.broken);
        save(&env, &loaded.settings).unwrap();
        let backup = p.with_file_name("settings.json.bak");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "{not json");
    }
}
