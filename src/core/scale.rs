//! `xrizer_scale` resolution and safe writing.
//!
//! The game reads the first candidate that parses as a number and clamps it
//! into `0.1..=4.0`. This module mirrors that order and writes only the
//! effective file, preferring a persistent path when creating a new one.

use std::collections::hash_map::DefaultHasher;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::core::atomic::{BackupPolicy, can_write_into, write_atomic};
use crate::core::env::{Env, scale_candidates};

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleState {
    Valid { raw: f32, value: f32 },
    Invalid { error: String },
    Missing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleCandidate {
    pub path: PathBuf,
    pub state: ScaleState,
    pub writable: bool,
    pub persistent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveSource {
    Existing,
    NewFile,
}

#[derive(Debug, Clone)]
pub struct ScaleSnapshot {
    pub candidates: Vec<ScaleCandidate>,
    pub effective: Option<PathBuf>,
    pub effective_value: Option<f32>,
    pub source: Option<EffectiveSource>,
    pub persistent_fallback: Option<(PathBuf, f32)>,
    pub notices: Vec<String>,
    pub fingerprint: u64,
}

#[derive(Debug, Clone)]
pub struct ScaleSaveOutcome {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
    pub previous_value: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleSaveError {
    Conflict { path: PathBuf, current_value: f32 },
    NoTarget { reason: String },
    Io(String),
}

/// Normalize a raw scale exactly like XRizer's `maybe_reload_scale`.
///
/// Values above 4.0 are interpreted as percentages (`70` -> `0.70`). The
/// result is always finite and within `0.1..=4.0`.
pub fn normalize_value(raw: f32) -> (f32, Vec<String>) {
    let mut notices = Vec::new();
    let mut value = raw;
    if raw > 4.0 {
        value = raw / 100.0;
        notices.push(format!("{raw} を {value:.3} として解釈しました"));
    }
    if value.is_nan() {
        value = 0.1;
        notices.push("NaN を 0.1 として扱います".to_string());
    } else if value == f32::INFINITY {
        value = 4.0;
        notices.push("+Infinity を 4.0 に丸めました".to_string());
    } else if value == f32::NEG_INFINITY {
        value = 0.1;
        notices.push("-Infinity を 0.1 に丸めました".to_string());
    }
    let clamped = value.clamp(0.1, 4.0);
    if (clamped - value).abs() > f32::EPSILON {
        notices.push(format!(
            "{value} を {clamped} に丸めました（範囲 0.1〜4.0）"
        ));
    }
    (clamped, notices)
}

pub fn parse_value(text: &str) -> Result<(f32, Vec<String>), String> {
    let raw = text
        .trim()
        .parse::<f32>()
        .map_err(|error| format!("数値として読めません: {error}"))?;
    Ok(normalize_value(raw))
}

pub fn format_value(value: f32) -> String {
    format!("{value:.3}")
}

/// A path is persistent unless it is on the runtime dir or the `/tmp`
/// fallback. Paths under `$HOME` win over the generic `/tmp` check so tests
/// and unusual setups with a home below `/tmp` still behave sensibly.
pub fn is_persistent(path: &Path, env: &Env) -> bool {
    if let Some(runtime) = &env.xdg_runtime_dir
        && path.starts_with(runtime)
    {
        return false;
    }
    if let Some(home) = &env.home
        && path.starts_with(home)
    {
        return true;
    }
    if path.starts_with("/tmp") {
        return false;
    }
    true
}

pub fn snapshot(env: &Env) -> ScaleSnapshot {
    snapshot_from_candidates(&scale_candidates(env), env)
}

/// Build a snapshot from an explicit candidate list. Tests use this to avoid
/// depending on a real `/tmp/xrizer_scale`.
pub fn snapshot_from_candidates(candidates: &[PathBuf], env: &Env) -> ScaleSnapshot {
    let mut entries = Vec::with_capacity(candidates.len());
    let mut notices = Vec::new();

    for path in candidates {
        let state = match std::fs::read_to_string(path) {
            Ok(text) => match text.trim().parse::<f32>() {
                Ok(raw) => {
                    let (value, parse_notices) = normalize_value(raw);
                    for notice in parse_notices {
                        notices.push(format!("{}: {notice}", path.display()));
                    }
                    ScaleState::Valid { raw, value }
                }
                Err(error) => ScaleState::Invalid {
                    error: format!("数値として読めません: {error}"),
                },
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ScaleState::Missing,
            Err(error) => ScaleState::Invalid {
                error: format!("読めません: {error}"),
            },
        };
        let writable = match &state {
            ScaleState::Missing => path.parent().is_some_and(can_write_into),
            _ => OpenOptions::new().write(true).open(path).is_ok(),
        };
        let persistent = is_persistent(path, env);
        entries.push(ScaleCandidate {
            path: path.clone(),
            state,
            writable,
            persistent,
        });
    }

    let mut effective = None;
    let mut effective_value = None;
    let mut source = None;
    let mut persistent_fallback = None;
    let mut effective_index = None;

    for (index, candidate) in entries.iter().enumerate() {
        if let ScaleState::Valid { value, .. } = &candidate.state {
            effective = Some(candidate.path.clone());
            effective_value = Some(*value);
            source = Some(EffectiveSource::Existing);
            effective_index = Some(index);
            break;
        }
    }

    if let Some(index) = effective_index {
        if !entries[index].persistent {
            for candidate in entries.iter().skip(index + 1) {
                if candidate.persistent
                    && let ScaleState::Valid { value, .. } = &candidate.state
                {
                    persistent_fallback = Some((candidate.path.clone(), *value));
                    notices.push(format!(
                        "ログアウト後は {} の値 {:.3} が使われます",
                        candidate.path.display(),
                        value
                    ));
                    break;
                }
            }
        }
    } else {
        let persistent = entries.iter().find(|candidate| {
            candidate.persistent
                && candidate.writable
                && matches!(&candidate.state, ScaleState::Missing)
        });
        let chosen = persistent.or_else(|| {
            entries.iter().find(|candidate| {
                candidate.writable && matches!(&candidate.state, ScaleState::Missing)
            })
        });
        if let Some(candidate) = chosen {
            effective = Some(candidate.path.clone());
            source = Some(EffectiveSource::NewFile);
            if !candidate.persistent {
                notices.push(format!(
                    "{} は揮発性のパスです。ログアウトで値が失われる可能性があります",
                    candidate.path.display()
                ));
            }
        } else {
            notices.push(
                "有効なスケールファイルがなく、書き込み可能な未作成候補もありません".to_string(),
            );
        }
    }

    if let Some(effective_path) = &effective {
        for candidate in &entries {
            if candidate.path.as_path() == effective_path.as_path() {
                break;
            }
            if matches!(&candidate.state, ScaleState::Invalid { .. }) {
                notices.push(format!(
                    "上位の {} は不正なため無視されます",
                    candidate.path.display()
                ));
            }
        }
    }

    let mut hasher = DefaultHasher::new();
    for candidate in &entries {
        candidate.path.hash(&mut hasher);
        candidate.writable.hash(&mut hasher);
        candidate.persistent.hash(&mut hasher);
        match &candidate.state {
            ScaleState::Valid { raw, value } => {
                2u8.hash(&mut hasher);
                raw.to_bits().hash(&mut hasher);
                value.to_bits().hash(&mut hasher);
            }
            ScaleState::Invalid { error } => {
                1u8.hash(&mut hasher);
                error.hash(&mut hasher);
            }
            ScaleState::Missing => 0u8.hash(&mut hasher),
        }
    }

    ScaleSnapshot {
        candidates: entries,
        effective,
        effective_value,
        source,
        persistent_fallback,
        notices,
        fingerprint: hasher.finish(),
    }
}

pub fn save(
    env: &Env,
    value: f32,
    expected_fingerprint: Option<u64>,
) -> Result<ScaleSaveOutcome, ScaleSaveError> {
    let (value, _) = normalize_value(value);
    let snap = snapshot(env);

    if let Some(expected) = expected_fingerprint
        && expected != snap.fingerprint
    {
        return Err(ScaleSaveError::Conflict {
            path: snap.effective.unwrap_or_default(),
            current_value: snap.effective_value.unwrap_or_default(),
        });
    }

    let Some(target) = snap.effective.clone() else {
        return Err(ScaleSaveError::NoTarget {
            reason: snap.notices.join(" / "),
        });
    };
    let previous_value = snap.effective_value;
    let text = format_value(value);
    let outcome = write_atomic(&target, text.as_bytes(), BackupPolicy::BeforeReplace)
        .map_err(ScaleSaveError::Io)?;

    let written = std::fs::read_to_string(&target)
        .map_err(|error| ScaleSaveError::Io(format!("保存後の再読込に失敗しました: {error}")))?;
    let (parsed, _) = parse_value(&written).map_err(ScaleSaveError::Io)?;
    if (parsed - value).abs() > 0.0005 {
        return Err(ScaleSaveError::Io(format!(
            "保存後の値が一致しません: 期待 {value}, 実際 {parsed}"
        )));
    }

    Ok(ScaleSaveOutcome {
        path: outcome.path,
        backup: outcome.backup,
        previous_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::Env;

    fn env_with(home: &std::path::Path, runtime: &std::path::Path) -> Env {
        Env {
            home: Some(home.into()),
            xdg_runtime_dir: Some(runtime.into()),
            ..Default::default()
        }
    }

    #[test]
    fn first_valid_file_wins_even_with_invalid_higher_priority() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(dir.path().join(".config")).unwrap();
        std::fs::write(runtime.join("xrizer_scale"), "broken").unwrap();
        std::fs::write(dir.path().join(".config/xrizer_scale"), "0.7").unwrap();
        let snap = snapshot(&env_with(dir.path(), &runtime));
        assert_eq!(
            snap.effective.unwrap(),
            dir.path().join(".config/xrizer_scale")
        );
        assert_eq!(snap.effective_value, Some(0.7));
        assert!(matches!(
            snap.candidates[0].state,
            ScaleState::Invalid { .. }
        ));
        assert_eq!(
            snap.candidates[1].state,
            ScaleState::Valid {
                raw: 0.7,
                value: 0.7
            }
        );
    }

    #[test]
    fn percent_and_out_of_range_values_are_normalized() {
        assert_eq!(normalize_value(70.0).0, 0.7);
        assert_eq!(normalize_value(5.0).0, 0.1);
        assert_eq!(normalize_value(-1.0).0, 0.1);
        assert_eq!(normalize_value(f32::NAN).0, 0.1);
        assert_eq!(normalize_value(f32::INFINITY).0, 4.0);
    }

    #[test]
    fn new_file_prefers_persistent_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        let env = env_with(dir.path(), &runtime);
        let candidates = vec![
            runtime.join("xrizer_scale"),
            dir.path().join(".config/xrizer_scale"),
            dir.path().join(".local/share/xrizer_scale"),
        ];
        let snap = snapshot_from_candidates(&candidates, &env);
        assert_eq!(
            snap.effective.unwrap(),
            dir.path().join(".config/xrizer_scale")
        );
        assert_eq!(snap.source, Some(EffectiveSource::NewFile));
    }

    #[test]
    fn refuses_to_overwrite_when_all_existing_candidates_are_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let paths = vec![
            dir.path().join("override"),
            dir.path().join("run/xrizer_scale"),
            dir.path().join(".config/xrizer_scale"),
            dir.path().join(".local/share/xrizer_scale"),
        ];
        for path in &paths {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "bad").unwrap();
        }
        let env = Env {
            home: Some(dir.path().into()),
            ..Default::default()
        };
        let snap = snapshot_from_candidates(&paths, &env);
        assert!(snap.effective.is_none());
        assert_eq!(snap.source, None);
    }

    #[test]
    fn saves_to_effective_path_with_backup_and_parses_back() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(runtime.join("xrizer_scale"), "0.500").unwrap();
        let env = env_with(dir.path(), &runtime);
        let first = snapshot(&env);
        let outcome = save(&env, 0.7, Some(first.fingerprint)).unwrap();
        assert_eq!(outcome.path, runtime.join("xrizer_scale"));
        assert_eq!(std::fs::read_to_string(&outcome.path).unwrap(), "0.700");
        assert_eq!(outcome.backup.unwrap(), runtime.join("xrizer_scale.bak"));
        let second = snapshot(&env);
        assert_eq!(second.effective_value, Some(0.7));
    }

    #[test]
    fn save_returns_conflict_when_the_file_changed_after_load() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        let path = runtime.join("xrizer_scale");
        std::fs::write(&path, "0.500").unwrap();
        let env = env_with(dir.path(), &runtime);
        let loaded = snapshot(&env);
        std::fs::write(&path, "0.900").unwrap();
        let err = save(&env, 0.7, Some(loaded.fingerprint)).unwrap_err();
        assert!(matches!(err, ScaleSaveError::Conflict { .. }));
    }

    #[test]
    fn volatile_effective_path_reports_persistent_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(dir.path().join(".config")).unwrap();
        std::fs::write(runtime.join("xrizer_scale"), "0.600").unwrap();
        std::fs::write(dir.path().join(".config/xrizer_scale"), "0.800").unwrap();
        let snap = snapshot(&env_with(dir.path(), &runtime));
        assert_eq!(snap.effective.unwrap(), runtime.join("xrizer_scale"));
        assert_eq!(
            snap.persistent_fallback,
            Some((dir.path().join(".config/xrizer_scale"), 0.8))
        );
    }
}
