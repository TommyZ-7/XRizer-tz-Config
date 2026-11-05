//! Path and environment resolution.
//!
//! The order mirrors XRizer-tz-Custom `src/upscaler/mod.rs` and
//! `src/settings.rs` so the GUI edits the file the game actually reads.

use std::path::PathBuf;

/// Environment inputs used by the resolver.
///
/// Keeping these in a struct instead of reading `std::env` everywhere makes
/// path-order tests deterministic and lets the layer installer pass an
/// explicit data home to its child process.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub xdg_runtime_dir: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub xrizer_upscaler_config: Option<PathBuf>,
    pub xrizer_scale_file: Option<PathBuf>,
}

impl Env {
    /// Read the current process environment. Empty values are treated as unset.
    pub fn from_process() -> Self {
        Self {
            home: env_path("HOME"),
            xdg_config_home: env_path("XDG_CONFIG_HOME"),
            xdg_runtime_dir: env_path("XDG_RUNTIME_DIR"),
            xdg_data_home: env_path("XDG_DATA_HOME"),
            xdg_cache_home: env_path("XDG_CACHE_HOME"),
            xrizer_upscaler_config: env_path("XRIZER_UPSCALER_CONFIG"),
            xrizer_scale_file: env_path("XRIZER_SCALE_FILE"),
        }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

/// Config file candidates in XRizer priority order.
pub fn config_candidates(env: &Env) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = &env.xrizer_upscaler_config {
        push_unique(&mut paths, path.clone());
    }
    if let Some(dir) = &env.xdg_config_home {
        push_unique(&mut paths, dir.join("xrizer/upscaler.json"));
    }
    if let Some(home) = &env.home {
        push_unique(&mut paths, home.join(".config/xrizer/upscaler.json"));
    }
    if let Some(dir) = &env.xdg_runtime_dir {
        push_unique(&mut paths, dir.join("xrizer_upscaler.json"));
    }
    push_unique(&mut paths, PathBuf::from("/tmp/xrizer_upscaler.json"));
    paths
}

/// Scale file candidates in XRizer priority order.
pub fn scale_candidates(env: &Env) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = &env.xrizer_scale_file {
        push_unique(&mut paths, path.clone());
    }
    if let Some(dir) = &env.xdg_runtime_dir {
        push_unique(&mut paths, dir.join("xrizer_scale"));
    }
    if let Some(home) = &env.home {
        push_unique(&mut paths, home.join(".config/xrizer_scale"));
        push_unique(&mut paths, home.join(".local/share/xrizer_scale"));
    }
    push_unique(&mut paths, PathBuf::from("/tmp/xrizer_scale"));
    paths
}

/// XDG data home, falling back to `$HOME/.local/share`.
pub fn data_home(env: &Env) -> Option<PathBuf> {
    if let Some(dir) = &env.xdg_data_home {
        return Some(dir.clone());
    }
    env.home.as_ref().map(|home| home.join(".local/share"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_candidates_follow_xrizer_order_and_dedup() {
        let env = Env {
            home: Some("/home/u".into()),
            xdg_config_home: Some("/home/u/.config".into()),
            xdg_runtime_dir: Some("/run/user/1000".into()),
            xrizer_upscaler_config: Some("/home/u/.config/xrizer/upscaler.json".into()),
            ..Default::default()
        };
        assert_eq!(
            config_candidates(&env),
            vec![
                PathBuf::from("/home/u/.config/xrizer/upscaler.json"),
                PathBuf::from("/run/user/1000/xrizer_upscaler.json"),
                PathBuf::from("/tmp/xrizer_upscaler.json"),
            ]
        );
    }

    #[test]
    fn scale_candidates_follow_xrizer_order() {
        let env = Env {
            home: Some("/home/u".into()),
            xdg_runtime_dir: Some("/run/user/1000".into()),
            ..Default::default()
        };
        assert_eq!(
            scale_candidates(&env),
            vec![
                PathBuf::from("/run/user/1000/xrizer_scale"),
                PathBuf::from("/home/u/.config/xrizer_scale"),
                PathBuf::from("/home/u/.local/share/xrizer_scale"),
                PathBuf::from("/tmp/xrizer_scale"),
            ]
        );
    }

    #[test]
    fn data_home_prefers_xdg_then_home() {
        assert_eq!(
            data_home(&Env {
                xdg_data_home: Some("/data".into()),
                home: Some("/home/u".into()),
                ..Default::default()
            }),
            Some("/data".into())
        );
        assert_eq!(
            data_home(&Env {
                home: Some("/home/u".into()),
                ..Default::default()
            }),
            Some("/home/u/.local/share".into())
        );
        assert_eq!(data_home(&Env::default()), None);
    }
}
