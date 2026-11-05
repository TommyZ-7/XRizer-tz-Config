//! Atomic file writing.
//!
//! A replacement is written to a temporary file in the same directory,
//! fsynced, optionally backed up, and renamed over the target. A crash or
//! write error therefore never leaves a half-written config behind.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupPolicy {
    None,
    BeforeReplace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
}

/// Replace `path` atomically. Symlinks are resolved so the link itself is
/// preserved; the file at the link target is replaced.
pub fn write_atomic(
    path: &Path,
    bytes: &[u8],
    backup: BackupPolicy,
) -> Result<WriteOutcome, String> {
    let target = resolve_target(path)?;
    let parent = target
        .parent()
        .ok_or_else(|| format!("{} に親ディレクトリがありません", target.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("{} を作成できません: {error}", parent.display()))?;

    if let Ok(metadata) = fs::symlink_metadata(&target)
        && !metadata.file_type().is_file()
    {
        return Err(format!(
            "{} は通常ファイルではないため置き換えません",
            target.display()
        ));
    }

    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "{} に一時ファイルを作成できません: {error}",
            parent.display()
        )
    })?;
    temp.write_all(bytes)
        .map_err(|error| format!("一時ファイルへ書き込めません: {error}"))?;
    temp.as_file_mut()
        .flush()
        .map_err(|error| format!("一時ファイルを flush できません: {error}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| format!("一時ファイルを fsync できません: {error}"))?;

    if let Ok(metadata) = fs::metadata(&target) {
        temp.as_file()
            .set_permissions(metadata.permissions())
            .map_err(|error| format!("パーミッションを設定できません: {error}"))?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o644))
                .map_err(|error| format!("パーミッションを設定できません: {error}"))?;
        }
    }

    let backup_path = if backup == BackupPolicy::BeforeReplace && target.exists() {
        let backup_path = backup_path_for(&target)?;
        fs::copy(&target, &backup_path).map_err(|error| {
            format!(
                "{} のバックアップ {} を作成できません: {error}",
                target.display(),
                backup_path.display()
            )
        })?;
        File::open(&backup_path)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("バックアップを fsync できません: {error}"))?;
        Some(backup_path)
    } else {
        None
    };

    temp.persist(&target)
        .map_err(|error| format!("{} を置き換えられません: {}", target.display(), error.error))?;

    // The rename is already durable enough for normal use; a directory fsync
    // is a best-effort improvement for power-loss cases.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(WriteOutcome {
        path: target,
        backup: backup_path,
    })
}

/// Check whether a file could be created below `dir` without creating the
/// directory chain. The nearest existing ancestor is probed with a temporary
/// file, so ACLs and read-only mounts are respected.
pub fn can_write_into(dir: &Path) -> bool {
    let mut ancestor = dir;
    while !ancestor.exists() {
        match ancestor.parent() {
            Some(parent) => ancestor = parent,
            None => return false,
        }
    }
    if !ancestor.is_dir() {
        return false;
    }
    tempfile::NamedTempFile::new_in(ancestor).is_ok()
}

fn resolve_target(path: &Path) -> Result<PathBuf, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::canonicalize(path)
            .map_err(|error| format!("{} のリンク先を解決できません: {error}", path.display())),
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(error) => Err(format!("{} を確認できません: {error}", path.display())),
    }
}

fn backup_path_for(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} にファイル名がありません", path.display()))?;
    Ok(path.with_file_name(format!("{}.bak", name.to_string_lossy())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_new_file_and_preserves_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b.json");
        let outcome = write_atomic(&path, b"hello", BackupPolicy::None).unwrap();
        assert_eq!(outcome.path, path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        assert!(outcome.backup.is_none());
    }

    #[test]
    fn replaces_existing_file_and_creates_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upscaler.json");
        std::fs::write(&path, "old").unwrap();
        let outcome = write_atomic(&path, b"new", BackupPolicy::BeforeReplace).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        let backup = outcome.backup.unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "old");
    }

    #[test]
    fn follows_symlink_and_keeps_the_link() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.json");
        let link = dir.path().join("link.json");
        std::fs::write(&target, "old").unwrap();
        symlink(&target, &link).unwrap();
        write_atomic(&link, b"new", BackupPolicy::None).unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn refuses_to_replace_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("target");
        std::fs::create_dir(&path).unwrap();
        assert!(write_atomic(&path, b"x", BackupPolicy::None).is_err());
    }

    #[test]
    fn can_write_into_missing_chain_uses_existing_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_write_into(&dir.path().join("missing/child")));
    }
}
