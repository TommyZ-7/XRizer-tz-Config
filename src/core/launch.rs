//! Steam launch option generation.
//!
//! Only the capture/layer environment variables requested by the user are
//! emitted. Values are quoted for POSIX `sh` because Steam inserts the string
//! into a shell command line, and `%command%` is always kept last.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::env::Env;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LayerMode {
    #[default]
    Unspecified,
    Enable,
    Disable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    #[default]
    Interval,
    Shutter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchForm {
    pub layer_mode: LayerMode,
    pub capture_enabled: bool,
    pub capture_dir: String,
    pub capture_mode: CaptureMode,
    pub interval_ms: u64,
    pub max_pairs: u32,
    pub scene: String,
    pub hold_ms: u64,
    pub lockout_ms: u64,
    pub pin_config_path: bool,
    pub pin_scale_path: bool,
    pub pin_layer_path: bool,
}

impl Default for LaunchForm {
    fn default() -> Self {
        Self {
            layer_mode: LayerMode::Unspecified,
            capture_enabled: false,
            capture_dir: String::new(),
            capture_mode: CaptureMode::Interval,
            interval_ms: 500,
            max_pairs: 300,
            scene: String::new(),
            hold_ms: 250,
            lockout_ms: 1000,
            pin_config_path: false,
            pin_scale_path: false,
            pin_layer_path: false,
        }
    }
}

impl LaunchForm {
    /// Clamp numeric fields into the ranges the game itself enforces.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut notices = Vec::new();
        clamp_u64(
            &mut self.interval_ms,
            100,
            60_000,
            "XRIZER_CAPTURE_INTERVAL_MS",
            &mut notices,
        );
        let max_pairs = self.max_pairs.clamp(1, 100_000);
        if max_pairs != self.max_pairs {
            notices.push(format!(
                "XRIZER_CAPTURE_MAX_PAIRS を {max_pairs} に丸めました（範囲 1〜100000）"
            ));
            self.max_pairs = max_pairs;
        }
        clamp_u64(
            &mut self.hold_ms,
            100,
            500,
            "XRIZER_CAPTURE_HOLD_MS",
            &mut notices,
        );
        clamp_u64(
            &mut self.lockout_ms,
            200,
            5_000,
            "XRIZER_CAPTURE_LOCKOUT_MS",
            &mut notices,
        );
        notices
    }
}

fn clamp_u64(value: &mut u64, min: u64, max: u64, name: &str, notices: &mut Vec<String>) {
    let clamped = (*value).clamp(min, max);
    if clamped != *value {
        notices.push(format!(
            "{name} を {clamped} に丸めました（範囲 {min}〜{max}）"
        ));
        *value = clamped;
    }
}

pub struct LaunchContext<'a> {
    pub env: &'a Env,
    pub config_path: &'a Path,
    pub scale_path: Option<&'a Path>,
    pub layer_manifest_dir: Option<&'a Path>,
    pub layer_installed: bool,
    pub preset_is_ml: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutput {
    pub command: String,
    pub env_only: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn generate(form: &LaunchForm, ctx: &LaunchContext<'_>) -> LaunchOutput {
    let mut vars: Vec<(String, String)> = Vec::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if form.pin_config_path {
        vars.push((
            "XRIZER_UPSCALER_CONFIG".to_string(),
            ctx.config_path.display().to_string(),
        ));
    }
    if form.pin_scale_path {
        if let Some(scale) = ctx.scale_path {
            vars.push(("XRIZER_SCALE_FILE".to_string(), scale.display().to_string()));
        } else {
            warnings.push(
                "スケールファイルの実効パスがないため、XRIZER_SCALE_FILE を追加できません"
                    .to_string(),
            );
        }
    }
    if form.layer_mode == LayerMode::Enable && form.pin_layer_path {
        if let Some(dir) = ctx.layer_manifest_dir {
            vars.push((
                "VK_ADD_IMPLICIT_LAYER_PATH".to_string(),
                dir.display().to_string(),
            ));
        } else {
            warnings.push(
                "Vulkan レイヤーの manifest ディレクトリがないため、VK_ADD_IMPLICIT_LAYER_PATH を追加できません"
                    .to_string(),
            );
        }
    }
    match form.layer_mode {
        LayerMode::Enable => {
            vars.push(("XRIZER_UPSCALER_VK_LAYER".to_string(), "1".to_string()));
            if !ctx.layer_installed {
                warnings.push(
                    "Vulkan レイヤーが未導入です。ml は FSR にフォールバックします".to_string(),
                );
            }
        }
        LayerMode::Disable => {
            vars.push((
                "XRIZER_UPSCALER_VK_LAYER_DISABLE".to_string(),
                "1".to_string(),
            ));
        }
        LayerMode::Unspecified => {}
    }
    if ctx.preset_is_ml && form.layer_mode != LayerMode::Enable {
        warnings.push("ml プリセットには Vulkan レイヤーの有効化が必要です".to_string());
    }

    if form.capture_enabled {
        match expand_tilde(&form.capture_dir, ctx.env.home.as_deref()) {
            Err(error) => errors.push(error),
            Ok(dir) if dir.is_empty() => {
                errors.push("キャプチャ保存先を指定してください".to_string());
            }
            Ok(dir) if !Path::new(&dir).is_absolute() => {
                errors.push("キャプチャ保存先は絶対パスで指定してください".to_string());
            }
            Ok(dir) if dir.contains('\0') || dir.contains('\n') || dir.contains('\r') => {
                errors.push("キャプチャ保存先に改行や NUL を含められません".to_string());
            }
            Ok(dir) => {
                vars.push(("XRIZER_CAPTURE".to_string(), "1".to_string()));
                vars.push(("XRIZER_CAPTURE_DIR".to_string(), dir));
                vars.push((
                    "XRIZER_CAPTURE_MODE".to_string(),
                    match form.capture_mode {
                        CaptureMode::Interval => "interval".to_string(),
                        CaptureMode::Shutter => "shutter".to_string(),
                    },
                ));
                match form.capture_mode {
                    CaptureMode::Interval => vars.push((
                        "XRIZER_CAPTURE_INTERVAL_MS".to_string(),
                        form.interval_ms.to_string(),
                    )),
                    CaptureMode::Shutter => {
                        vars.push((
                            "XRIZER_CAPTURE_HOLD_MS".to_string(),
                            form.hold_ms.to_string(),
                        ));
                        vars.push((
                            "XRIZER_CAPTURE_LOCKOUT_MS".to_string(),
                            form.lockout_ms.to_string(),
                        ));
                    }
                }
                vars.push((
                    "XRIZER_CAPTURE_MAX_PAIRS".to_string(),
                    form.max_pairs.to_string(),
                ));
                let scene = form.scene.trim();
                if !scene.is_empty() {
                    vars.push(("XRIZER_CAPTURE_SCENE".to_string(), scene.to_string()));
                }
            }
        }
    }

    let mut quoted_vars = Vec::with_capacity(vars.len());
    for (key, value) in vars {
        match shell_quote(&value) {
            Ok(quoted) => quoted_vars.push(format!("{key}={quoted}")),
            Err(error) => errors.push(format!("{key}: {error}")),
        }
    }
    if !errors.is_empty() {
        return LaunchOutput {
            command: String::new(),
            env_only: String::new(),
            errors,
            warnings,
        };
    }
    let env_only = quoted_vars.join(" ");
    let command = if env_only.is_empty() {
        "%command%".to_string()
    } else {
        format!("{env_only} %command%")
    };
    LaunchOutput {
        command,
        env_only,
        errors,
        warnings,
    }
}

/// Quote a value for a POSIX shell. Simple values stay readable; anything
/// else is single-quoted, with embedded single quotes escaped.
pub fn shell_quote(value: &str) -> Result<String, String> {
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return Err("改行や NUL を含む値は起動オプションに使えません".to_string());
    }
    if value.is_empty() {
        return Ok("''".to_string());
    }
    let safe = value.chars().all(|ch| {
        ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '_' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',' | '-'
            )
    });
    if safe {
        Ok(value.to_string())
    } else {
        Ok(format!("'{}'", value.replace('\'', "'\\''")))
    }
}

/// Expand a leading `~` or `~/` using the GUI's home. `~user` is rejected
/// because the game process may have a different home.
pub fn expand_tilde(value: &str, home: Option<&Path>) -> Result<String, String> {
    if value == "~" {
        return home
            .map(|home| home.display().to_string())
            .ok_or_else(|| "HOME が未設定のため ~ を展開できません".to_string());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        let home = home.ok_or_else(|| "HOME が未設定のため ~ を展開できません".to_string())?;
        return Ok(home.join(rest).display().to_string());
    }
    if value.starts_with('~') {
        return Err(
            "~user 形式のパスは未対応です。$HOME からの相対パスを使ってください".to_string(),
        );
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::Env;
    use std::path::Path;

    fn ctx<'a>(env: &'a Env, config: &'a Path) -> LaunchContext<'a> {
        LaunchContext {
            env,
            config_path: config,
            scale_path: None,
            layer_manifest_dir: None,
            layer_installed: false,
            preset_is_ml: false,
        }
    }

    #[test]
    fn default_output_is_just_command() {
        let env = Env::default();
        let config = Path::new("/home/u/.config/xrizer/upscaler.json");
        let out = generate(&LaunchForm::default(), &ctx(&env, config));
        assert_eq!(out.command, "%command%");
        assert!(out.errors.is_empty());
    }

    #[test]
    fn quotes_spaces_quotes_and_dollars() {
        assert_eq!(shell_quote("0.700").unwrap(), "0.700");
        assert_eq!(shell_quote("a b").unwrap(), "'a b'");
        assert_eq!(shell_quote("it's").unwrap(), "'it'\\''s'");
        assert_eq!(shell_quote("$HOME").unwrap(), "'$HOME'");
        assert!(shell_quote("a\nb").is_err());
    }

    #[test]
    fn generates_capture_and_layer_options_in_order() {
        let env = Env {
            home: Some("/home/u".into()),
            ..Default::default()
        };
        let config = Path::new("/home/u/.config/xrizer/upscaler.json");
        let form = LaunchForm {
            layer_mode: LayerMode::Enable,
            capture_enabled: true,
            capture_dir: "/home/u/captures".into(),
            capture_mode: CaptureMode::Interval,
            interval_ms: 500,
            max_pairs: 300,
            scene: "scene-a".into(),
            ..Default::default()
        };
        let out = generate(&form, &ctx(&env, config));
        assert_eq!(
            out.command,
            "XRIZER_UPSCALER_VK_LAYER=1 XRIZER_CAPTURE=1 XRIZER_CAPTURE_DIR=/home/u/captures XRIZER_CAPTURE_MODE=interval XRIZER_CAPTURE_INTERVAL_MS=500 XRIZER_CAPTURE_MAX_PAIRS=300 XRIZER_CAPTURE_SCENE=scene-a %command%"
        );
    }

    #[test]
    fn shutter_uses_hold_and_lockout() {
        let env = Env::default();
        let config = Path::new("/tmp/upscaler.json");
        let form = LaunchForm {
            capture_enabled: true,
            capture_dir: "/tmp/captures".into(),
            capture_mode: CaptureMode::Shutter,
            hold_ms: 250,
            lockout_ms: 1000,
            ..Default::default()
        };
        let out = generate(&form, &ctx(&env, config));
        assert!(out.command.contains("XRIZER_CAPTURE_HOLD_MS=250"));
        assert!(out.command.contains("XRIZER_CAPTURE_LOCKOUT_MS=1000"));
        assert!(!out.command.contains("XRIZER_CAPTURE_INTERVAL_MS"));
    }

    #[test]
    fn pins_paths_and_layer_manifest() {
        let env = Env::default();
        let config = Path::new("/home/u/.config/xrizer/upscaler.json");
        let scale = Path::new("/run/user/1000/xrizer_scale");
        let manifest = Path::new("/home/u/.local/share/vulkan/implicit_layer.d");
        let form = LaunchForm {
            layer_mode: LayerMode::Enable,
            pin_config_path: true,
            pin_scale_path: true,
            pin_layer_path: true,
            ..Default::default()
        };
        let out = generate(
            &form,
            &LaunchContext {
                env: &env,
                config_path: config,
                scale_path: Some(scale),
                layer_manifest_dir: Some(manifest),
                layer_installed: true,
                preset_is_ml: true,
            },
        );
        assert_eq!(
            out.command,
            "XRIZER_UPSCALER_CONFIG=/home/u/.config/xrizer/upscaler.json XRIZER_SCALE_FILE=/run/user/1000/xrizer_scale VK_ADD_IMPLICIT_LAYER_PATH=/home/u/.local/share/vulkan/implicit_layer.d XRIZER_UPSCALER_VK_LAYER=1 %command%"
        );
    }

    #[test]
    fn rejects_relative_and_newline_capture_dirs() {
        let env = Env::default();
        let config = Path::new("/tmp/upscaler.json");
        let form = LaunchForm {
            capture_enabled: true,
            capture_dir: "captures".into(),
            ..Default::default()
        };
        let out = generate(&form, &ctx(&env, config));
        assert!(!out.errors.is_empty());
        assert_eq!(out.command, "");

        let form = LaunchForm {
            capture_enabled: true,
            capture_dir: "/tmp/a\nb".into(),
            ..Default::default()
        };
        assert!(!generate(&form, &ctx(&env, config)).errors.is_empty());
    }

    #[test]
    fn shell_quote_roundtrips_through_sh() {
        let value = "a b'c$d`e";
        let quoted = shell_quote(value).unwrap();
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {quoted}"))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
    }

    #[test]
    fn sanitizes_numeric_ranges() {
        let mut form = LaunchForm {
            interval_ms: 1,
            max_pairs: 0,
            hold_ms: 9999,
            lockout_ms: 1,
            ..Default::default()
        };
        let notices = form.sanitize();
        assert_eq!(form.interval_ms, 100);
        assert_eq!(form.max_pairs, 1);
        assert_eq!(form.hold_ms, 500);
        assert_eq!(form.lockout_ms, 200);
        assert_eq!(notices.len(), 4);
    }

    #[test]
    fn expands_tilde_and_rejects_user_form() {
        let home = Path::new("/home/u");
        assert_eq!(expand_tilde("~", Some(home)).unwrap(), "/home/u");
        assert_eq!(
            expand_tilde("~/captures", Some(home)).unwrap(),
            "/home/u/captures"
        );
        assert_eq!(expand_tilde("/tmp/a", Some(home)).unwrap(), "/tmp/a");
        assert!(expand_tilde("~other/a", Some(home)).is_err());
        assert!(expand_tilde("~/a", None).is_err());
    }

    #[test]
    fn warns_when_ml_needs_the_layer() {
        let env = Env::default();
        let config = Path::new("/tmp/upscaler.json");
        let out = generate(
            &LaunchForm::default(),
            &LaunchContext {
                env: &env,
                config_path: config,
                scale_path: None,
                layer_manifest_dir: None,
                layer_installed: false,
                preset_is_ml: true,
            },
        );
        assert!(
            out.warnings
                .iter()
                .any(|warning| warning.contains("Vulkan"))
        );
    }
}
