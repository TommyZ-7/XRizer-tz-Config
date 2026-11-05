//! Vulkan layer installation.
//!
//! The installer script and manifest template are bundled from XRizer so the
//! GUI can install the opt-in layer without a local XRizer checkout. The
//! script is materialized into a temporary directory and run with an explicit
//! `XRIZER_LAYER_LIB`, `XDG_DATA_HOME`, and `HOME`.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::core::env::{Env, data_home};

const INSTALLER_SCRIPT: &str = include_str!("../../assets/install-vulkan-layer.sh");
const MANIFEST_TEMPLATE: &str = include_str!("../../assets/manifest.json.in");
const LAYER_NAME: &str = "VK_LAYER_XRIZER_upscaler";
const MANIFEST_NAME: &str = "xrizer_upscaler_layer.json";
const LIB_NAME: &str = "libxrizer_upscaler_layer.so";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerPaths {
    pub data_home: PathBuf,
    pub manifest_dir: PathBuf,
    pub manifest: PathBuf,
    pub lib_dir: PathBuf,
    pub lib: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerStatus {
    Installed {
        library_path: PathBuf,
        lib_size: u64,
    },
    Partial {
        detail: String,
    },
    NotInstalled,
    Error {
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSnapshot {
    pub paths: Option<LayerPaths>,
    pub status: LayerStatus,
}

#[derive(Debug, Clone)]
pub struct SoInfo {
    pub size: u64,
    pub hash: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
pub struct InstallResult {
    pub stdout: String,
    pub stderr: String,
    pub snapshot: LayerSnapshot,
}

pub fn layer_paths(env: &Env) -> Result<LayerPaths, String> {
    let data_home =
        data_home(env).ok_or_else(|| "XDG_DATA_HOME も HOME も設定されていません".to_string())?;
    let data_home = if data_home.is_absolute() {
        data_home
    } else {
        std::env::current_dir()
            .map_err(|error| format!("現在のディレクトリを取得できません: {error}"))?
            .join(data_home)
    };
    let manifest_dir = data_home.join("vulkan/implicit_layer.d");
    let manifest = manifest_dir.join(MANIFEST_NAME);
    let lib_dir = data_home.join("xrizer/vulkan-layer");
    let lib = lib_dir.join(LIB_NAME);
    Ok(LayerPaths {
        data_home,
        manifest_dir,
        manifest,
        lib_dir,
        lib,
    })
}

pub fn snapshot(env: &Env) -> LayerSnapshot {
    let paths = match layer_paths(env) {
        Ok(paths) => paths,
        Err(detail) => {
            return LayerSnapshot {
                paths: None,
                status: LayerStatus::Error { detail },
            };
        }
    };

    let manifest_exists = paths.manifest.exists();
    let lib_exists = paths.lib.exists();
    if !manifest_exists && !lib_exists {
        return LayerSnapshot {
            paths: Some(paths),
            status: LayerStatus::NotInstalled,
        };
    }
    if !manifest_exists {
        return LayerSnapshot {
            paths: Some(paths),
            status: LayerStatus::Partial {
                detail: "manifest がありません（ライブラリのみ）".to_string(),
            },
        };
    }

    let text = match fs::read_to_string(&paths.manifest) {
        Ok(text) => text,
        Err(error) => {
            return LayerSnapshot {
                paths: Some(paths),
                status: LayerStatus::Error {
                    detail: format!("manifest を読めません: {error}"),
                },
            };
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            return LayerSnapshot {
                paths: Some(paths),
                status: LayerStatus::Error {
                    detail: format!("manifest の JSON が不正です: {error}"),
                },
            };
        }
    };
    let layer = value.get("layer");
    let name = layer
        .and_then(|layer| layer.get("name"))
        .and_then(|value| value.as_str());
    if name != Some(LAYER_NAME) {
        return LayerSnapshot {
            paths: Some(paths),
            status: LayerStatus::Partial {
                detail: "manifest の layer.name が想定と違います".to_string(),
            },
        };
    }
    let Some(library_path) = layer
        .and_then(|layer| layer.get("library_path"))
        .and_then(|value| value.as_str())
    else {
        return LayerSnapshot {
            paths: Some(paths),
            status: LayerStatus::Partial {
                detail: "manifest に library_path がありません".to_string(),
            },
        };
    };
    let library_path = resolve_manifest_library(&paths, library_path);
    let lib_size = match fs::metadata(&library_path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => {
            return LayerSnapshot {
                paths: Some(paths),
                status: LayerStatus::Partial {
                    detail: format!("manifest の参照先 {} がありません", library_path.display()),
                },
            };
        }
    };

    LayerSnapshot {
        paths: Some(paths),
        status: LayerStatus::Installed {
            library_path,
            lib_size,
        },
    }
}

pub fn inspect_so(path: &Path) -> Result<SoInfo, String> {
    if !path.is_absolute() {
        return Err("Vulkan レイヤーの .so は絶対パスで指定してください".to_string());
    }
    let text = path.to_string_lossy();
    if text.contains('\n') || text.contains('\r') {
        return Err("Vulkan レイヤーのパスに改行を含められません".to_string());
    }
    let metadata = fs::metadata(path).map_err(|error| format!(".so を確認できません: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("{} は通常ファイルではありません", path.display()));
    }
    if metadata.len() == 0 {
        return Err(".so が空です".to_string());
    }
    let mut file = File::open(path).map_err(|error| format!(".so を開けません: {error}"))?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|error| format!(".so の先頭を読めません: {error}"))?;
    if &magic != b"\x7fELF" {
        return Err("選択したファイルは ELF ではありません".to_string());
    }
    let hash = hash_file(path)?;
    Ok(SoInfo {
        size: metadata.len(),
        hash,
        modified: metadata.modified().ok(),
    })
}

/// FNV-1a 64-bit. Used only for comparing the selected and installed `.so`;
/// `install` does not preserve mtime, so mtime is not a reliable indicator.
pub fn hash_file(path: &Path) -> Result<u64, String> {
    let mut file =
        File::open(path).map_err(|error| format!("ハッシュ対象を開けません: {error}"))?;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("ハッシュ対象を読めません: {error}"))?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Ok(hash)
}

pub fn auto_detect_so(env: &Env) -> Option<PathBuf> {
    let home = env.home.as_ref()?;
    let base = home.join("work-saeki/xrizer");
    [
        base.join("target/release/libxrizer_upscaler_layer.so"),
        base.join("target/debug/libxrizer_upscaler_layer.so"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// The bundled script inserts `$XDG_DATA_HOME` into a `sed` replacement. These
/// characters would change the replacement or break the quoted shell string.
pub fn validate_data_home_for_sed(data_home: &Path) -> Result<(), String> {
    let text = data_home.to_string_lossy();
    if text
        .chars()
        .any(|ch| matches!(ch, '|' | '&' | '\\' | '"' | '\n' | '\r'))
    {
        return Err(format!(
            "XDG_DATA_HOME ({}) にインストーラが扱えない文字が含まれています",
            data_home.display()
        ));
    }
    Ok(())
}

pub fn materialize_installer() -> Result<tempfile::TempDir, String> {
    let temp = tempfile::tempdir()
        .map_err(|error| format!("インストーラ用の一時ディレクトリを作成できません: {error}"))?;
    let scripts = temp.path().join("scripts");
    let vulkan = temp.path().join("vulkan-layer");
    fs::create_dir_all(&scripts)
        .map_err(|error| format!("{} を作成できません: {error}", scripts.display()))?;
    fs::create_dir_all(&vulkan)
        .map_err(|error| format!("{} を作成できません: {error}", vulkan.display()))?;
    let script = scripts.join("install-vulkan-layer.sh");
    fs::write(&script, INSTALLER_SCRIPT)
        .map_err(|error| format!("インストーラを書き出せません: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("インストーラに実行権限を設定できません: {error}"))?;
    }
    fs::write(vulkan.join("manifest.json.in"), MANIFEST_TEMPLATE)
        .map_err(|error| format!("manifest テンプレートを書き出せません: {error}"))?;
    Ok(temp)
}

pub fn install(env: &Env, so_path: &Path) -> Result<InstallResult, String> {
    let paths = layer_paths(env)?;
    validate_data_home_for_sed(&paths.data_home)?;
    inspect_so(so_path)?;
    let temp = materialize_installer()?;
    let (stdout, stderr) = run_script(env, &paths, &temp, "--user", Some(so_path))?;

    if let Err(detail) = verify_installed_manifest(&paths) {
        let _ = run_script(env, &paths, &temp, "--remove", None);
        return Err(format!(
            "インストール後の manifest 検証に失敗しました: {detail}"
        ));
    }
    let snapshot = snapshot(env);
    if !matches!(snapshot.status, LayerStatus::Installed { .. }) {
        let _ = run_script(env, &paths, &temp, "--remove", None);
        return Err(format!(
            "インストール後の状態が Installed ではありません: {:?}",
            snapshot.status
        ));
    }

    Ok(InstallResult {
        stdout,
        stderr,
        snapshot,
    })
}

pub fn remove(env: &Env) -> Result<InstallResult, String> {
    let paths = layer_paths(env)?;
    let temp = materialize_installer()?;
    let (stdout, stderr) = run_script(env, &paths, &temp, "--remove", None)?;
    Ok(InstallResult {
        stdout,
        stderr,
        snapshot: snapshot(env),
    })
}

fn run_script(
    env: &Env,
    paths: &LayerPaths,
    temp: &tempfile::TempDir,
    argument: &str,
    so_path: Option<&Path>,
) -> Result<(String, String), String> {
    let script = temp.path().join("scripts/install-vulkan-layer.sh");
    let mut command = Command::new("bash");
    command
        .arg(&script)
        .arg(argument)
        .env("XDG_DATA_HOME", &paths.data_home);
    if let Some(home) = &env.home {
        command.env("HOME", home);
    }
    if let Some(so_path) = so_path {
        command.env("XRIZER_LAYER_LIB", so_path);
    }
    let output = command
        .output()
        .map_err(|error| format!("インストーラを実行できません: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(format!(
            "インストーラが失敗しました (終了コード {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
            output.status.code()
        ));
    }
    Ok((stdout, stderr))
}

fn verify_installed_manifest(paths: &LayerPaths) -> Result<(), String> {
    let text = fs::read_to_string(&paths.manifest)
        .map_err(|error| format!("manifest を読めません: {error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("manifest が不正です: {error}"))?;
    let layer = value
        .get("layer")
        .ok_or_else(|| "manifest に layer がありません".to_string())?;
    if layer.get("name").and_then(|value| value.as_str()) != Some(LAYER_NAME) {
        return Err("manifest の layer.name が想定と違います".to_string());
    }
    let library = layer
        .get("library_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "manifest に library_path がありません".to_string())?;
    let library = resolve_manifest_library(paths, library);
    let expected = fs::canonicalize(&paths.lib).unwrap_or_else(|_| paths.lib.clone());
    let actual = fs::canonicalize(&library).unwrap_or(library);
    if actual != expected {
        return Err(format!(
            "manifest の library_path が想定と違います: {} (期待 {})",
            actual.display(),
            expected.display()
        ));
    }
    Ok(())
}

fn resolve_manifest_library(paths: &LayerPaths, library_path: &str) -> PathBuf {
    let path = PathBuf::from(library_path);
    if path.is_absolute() {
        path
    } else {
        paths.manifest_dir.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::Env;

    fn dummy_elf(path: &std::path::Path) {
        std::fs::write(path, b"\x7fELF\x01\x01\x01\x00dummy").unwrap();
    }

    #[test]
    fn inspects_elf_and_rejects_non_elf() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("lib.so");
        dummy_elf(&good);
        assert!(inspect_so(&good).is_ok());
        let bad = dir.path().join("bad.so");
        std::fs::write(&bad, b"not elf").unwrap();
        assert!(inspect_so(&bad).is_err());
    }

    #[test]
    fn rejects_sed_unsafe_data_home() {
        assert!(validate_data_home_for_sed(std::path::Path::new("/home/u/&bad")).is_err());
        assert!(validate_data_home_for_sed(std::path::Path::new("/home/u/|bad")).is_err());
        assert!(validate_data_home_for_sed(std::path::Path::new("/home/u/\\bad")).is_err());
        assert!(validate_data_home_for_sed(std::path::Path::new("/home/u/\"bad")).is_err());
        assert!(validate_data_home_for_sed(std::path::Path::new("/home/u/good")).is_ok());
    }

    #[test]
    fn snapshot_reports_not_installed_partial_and_error() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let env = Env {
            home: Some(dir.path().into()),
            xdg_data_home: Some(data),
            ..Default::default()
        };
        assert_eq!(snapshot(&env).status, LayerStatus::NotInstalled);
        let paths = layer_paths(&env).unwrap();
        std::fs::create_dir_all(&paths.manifest_dir).unwrap();
        std::fs::write(
            &paths.manifest,
            r#"{"layer":{"name":"VK_LAYER_XRIZER_upscaler","library_path":"/missing/lib.so"}}"#,
        )
        .unwrap();
        assert!(matches!(snapshot(&env).status, LayerStatus::Partial { .. }));
        std::fs::write(&paths.manifest, "{not json").unwrap();
        assert!(matches!(snapshot(&env).status, LayerStatus::Error { .. }));
    }

    #[test]
    fn installer_materializes_expected_tree() {
        let temp = materialize_installer().unwrap();
        assert!(temp.path().join("scripts/install-vulkan-layer.sh").exists());
        assert!(temp.path().join("vulkan-layer/manifest.json.in").exists());
    }

    #[test]
    fn installs_and_removes_in_a_temp_data_home() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let env = Env {
            home: Some(dir.path().into()),
            xdg_data_home: Some(data.clone()),
            ..Default::default()
        };
        let so = dir.path().join("libxrizer_upscaler_layer.so");
        dummy_elf(&so);
        let installed = install(&env, &so).unwrap();
        assert_eq!(
            installed.snapshot.status,
            LayerStatus::Installed {
                library_path: data.join("xrizer/vulkan-layer/libxrizer_upscaler_layer.so"),
                lib_size: std::fs::metadata(&so).unwrap().len(),
            }
        );
        assert!(
            data.join("vulkan/implicit_layer.d/xrizer_upscaler_layer.json")
                .exists()
        );
        let removed = remove(&env).unwrap();
        assert_eq!(removed.snapshot.status, LayerStatus::NotInstalled);
    }
}
