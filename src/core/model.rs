//! XRU1 model validation and placement.
//!
//! The GUI validates the header and layer table before copying a model, so a
//! truncated or malformed file is rejected before it can replace a working
//! `upscaler_model.xru`.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::core::env::Env;

pub const FEATURE_COUNT: u32 = 28;
const MAGIC: &[u8; 4] = b"XRU1";
const VERSION: u16 = 1;
const HEADER_SIZE: usize = 64;
const LAYER_ENTRY_SIZE: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Mlp,
    ResidualCnn,
    Offset,
}

impl ModelKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mlp => "MLP",
            Self::ResidualCnn => "Residual CNN",
            Self::Offset => "Offset",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    pub kind: ModelKind,
    pub feature_count: u32,
    pub hidden: u16,
    pub channels: u16,
    pub layer_count: usize,
    pub layer_shapes: Vec<(u32, u32)>,
    pub file_size: u64,
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInstallOutcome {
    pub target: PathBuf,
    pub backup: Option<PathBuf>,
}

pub fn default_model_path(env: &Env) -> PathBuf {
    env.home
        .as_ref()
        .map(|home| home.join(".config/xrizer/upscaler_model.xru"))
        .unwrap_or_else(|| PathBuf::from("/tmp/xrizer_upscaler_model.xru"))
}

pub fn inspect(path: &Path) -> Result<ModelInfo, String> {
    let metadata =
        fs::metadata(path).map_err(|error| format!("モデルを確認できません: {error}"))?;
    if !metadata.is_file() {
        return Err(format!("{} は通常ファイルではありません", path.display()));
    }
    if metadata.len() < HEADER_SIZE as u64 {
        return Err("XRU1 ヘッダが切り詰められています".to_string());
    }
    let mut file = File::open(path).map_err(|error| format!("モデルを開けません: {error}"))?;
    let mut header = [0u8; HEADER_SIZE];
    file.read_exact(&mut header)
        .map_err(|error| format!("XRU1 ヘッダを読めません: {error}"))?;
    let layer_count = read_u16(&header, 14)? as usize;
    let table_len = HEADER_SIZE + layer_count * LAYER_ENTRY_SIZE;
    let mut table = vec![0u8; table_len];
    table[..HEADER_SIZE].copy_from_slice(&header);
    file.read_exact(&mut table[HEADER_SIZE..])
        .map_err(|error| format!("XRU1 レイヤーテーブルを読めません: {error}"))?;
    validate_header(&table, metadata.len())
}

/// Validate a header plus layer table and the exact expected file size.
pub fn validate_header(header_and_table: &[u8], file_size: u64) -> Result<ModelInfo, String> {
    if header_and_table.len() < HEADER_SIZE {
        return Err("XRU1 ヘッダが切り詰められています".to_string());
    }
    if &header_and_table[0..4] != MAGIC {
        return Err("XRU1 のマジックが不正です".to_string());
    }
    let version = read_u16(header_and_table, 4)?;
    if version != VERSION {
        return Err(format!("未対応の XRU1 バージョン {version}"));
    }
    let kind = match header_and_table[6] {
        0 => ModelKind::Mlp,
        1 => ModelKind::ResidualCnn,
        2 => ModelKind::Offset,
        other => return Err(format!("未知のモデル種別 {other}")),
    };
    let feature_count = read_u16(header_and_table, 8)? as u32;
    let hidden = read_u16(header_and_table, 10)?;
    let channels = read_u16(header_and_table, 12)?;
    let layer_count = read_u16(header_and_table, 14)? as usize;
    if layer_count == 0 {
        return Err("XRU1 にレイヤーがありません".to_string());
    }
    let input_scale = read_f32(header_and_table, 16)?;
    let activation_scale = read_f32(header_and_table, 20)?;
    let residual_clamp = read_f32(header_and_table, 24)?;
    let scale = read_f32(header_and_table, 28)?;
    for (name, value) in [
        ("input_scale", input_scale),
        ("activation_scale", activation_scale),
        ("residual_clamp", residual_clamp),
        ("scale", scale),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(format!("{name} が不正です"));
        }
    }
    if residual_clamp > 1.0 {
        return Err("residual_clamp は 1.0 以下である必要があります".to_string());
    }

    let table_end = HEADER_SIZE
        .checked_add(
            layer_count
                .checked_mul(LAYER_ENTRY_SIZE)
                .ok_or_else(|| "レイヤーテーブルが大きすぎます".to_string())?,
        )
        .ok_or_else(|| "レイヤーテーブルが大きすぎます".to_string())?;
    if header_and_table.len() < table_end {
        return Err("XRU1 レイヤーテーブルが切り詰められています".to_string());
    }

    let mut offset = HEADER_SIZE;
    let mut shapes = Vec::with_capacity(layer_count);
    let mut expected_size = HEADER_SIZE as u64 + layer_count as u64 * LAYER_ENTRY_SIZE as u64;
    for _ in 0..layer_count {
        let input = read_u32(header_and_table, offset)?;
        let output = read_u32(header_and_table, offset + 4)?;
        let weight_scale = read_f32(header_and_table, offset + 8)?;
        let layer_input_scale = read_f32(header_and_table, offset + 12)?;
        let layer_output_scale = read_f32(header_and_table, offset + 16)?;
        offset += LAYER_ENTRY_SIZE;
        if input == 0 || output == 0 {
            return Err("レイヤーの次元が 0 です".to_string());
        }
        for (name, value) in [
            ("weight_scale", weight_scale),
            ("input_scale", layer_input_scale),
            ("output_scale", layer_output_scale),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("レイヤーの {name} が不正です"));
            }
        }
        shapes.push((input, output));
        expected_size = expected_size
            .checked_add(input as u64 * output as u64)
            .and_then(|value| value.checked_add(output as u64 * 4))
            .ok_or_else(|| "モデルサイズが大きすぎます".to_string())?;
    }

    if shapes[0].0 != feature_count {
        return Err("最初のレイヤー入力が feature_count と一致しません".to_string());
    }
    for pair in shapes.windows(2) {
        if pair[0].1 != pair[1].0 {
            return Err("レイヤーの次元が連結していません".to_string());
        }
    }

    match kind {
        ModelKind::Mlp => {
            if layer_count != 2 {
                return Err("MLP は 2 レイヤーである必要があります".to_string());
            }
            if feature_count != FEATURE_COUNT {
                return Err(format!("MLP は {FEATURE_COUNT} 個の入力特徴を期待します"));
            }
            if !matches!(hidden, 8 | 16) {
                return Err(format!("未対応の MLP hidden サイズ {hidden}"));
            }
            if shapes[0].1 != u32::from(hidden)
                || shapes[1].0 != u32::from(hidden)
                || shapes[1].1 != 1
            {
                return Err("MLP のレイヤー形状が不正です".to_string());
            }
        }
        ModelKind::ResidualCnn | ModelKind::Offset => {
            if layer_count != 3 {
                return Err("CNN / Offset は 3 レイヤーである必要があります".to_string());
            }
            if feature_count != FEATURE_COUNT {
                return Err(format!(
                    "CNN / Offset は {FEATURE_COUNT} 個の入力特徴を期待します"
                ));
            }
            if !matches!(channels, 8 | 16 | 32) {
                return Err(format!("未対応のチャンネル数 {channels}"));
            }
            let channels = u32::from(channels);
            if shapes[0].1 != channels
                || shapes[1].0 != channels
                || shapes[1].1 != channels
                || shapes[2].0 != channels
                || shapes[2].1 != 3
            {
                return Err("CNN / Offset のレイヤー形状が不正です".to_string());
            }
        }
    }

    if expected_size != file_size {
        return Err(format!(
            "モデルサイズが一致しません: 期待 {expected_size}, 実際 {file_size}"
        ));
    }

    Ok(ModelInfo {
        kind,
        feature_count,
        hidden,
        channels,
        layer_count,
        layer_shapes: shapes,
        file_size,
        scale,
    })
}

/// Copy a validated model into place. The old model is kept as `<name>.bak`.
pub fn install(source: &Path, target: &Path) -> Result<ModelInstallOutcome, String> {
    inspect(source)?;
    let source_canon = fs::canonicalize(source).ok();
    let target_canon = fs::canonicalize(target).ok();
    if source == target
        || (source_canon.is_some() && source_canon.as_ref() == target_canon.as_ref())
    {
        return Err("選択したモデルは既に既定パスにあります".to_string());
    }

    let parent = target
        .parent()
        .ok_or_else(|| format!("{} に親ディレクトリがありません", target.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("{} を作成できません: {error}", parent.display()))?;

    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "{} に一時ファイルを作成できません: {error}",
            parent.display()
        )
    })?;
    let mut input = File::open(source).map_err(|error| format!("モデルを開けません: {error}"))?;
    std::io::copy(&mut input, temp.as_file_mut())
        .map_err(|error| format!("モデルをコピーできません: {error}"))?;
    temp.as_file_mut()
        .flush()
        .map_err(|error| format!("モデルを flush できません: {error}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| format!("モデルを fsync できません: {error}"))?;
    inspect(temp.path())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(|error| format!("モデルのパーミッションを設定できません: {error}"))?;
    }

    let backup = if target.exists() {
        let name = target
            .file_name()
            .ok_or_else(|| format!("{} にファイル名がありません", target.display()))?;
        let backup_path = target.with_file_name(format!("{}.bak", name.to_string_lossy()));
        fs::copy(target, &backup_path)
            .map_err(|error| format!("既存モデルをバックアップできません: {error}"))?;
        File::open(&backup_path)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("バックアップを fsync できません: {error}"))?;
        Some(backup_path)
    } else {
        None
    };

    temp.persist(target)
        .map_err(|error| format!("{} を置き換えられません: {}", target.display(), error.error))?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(ModelInstallOutcome {
        target: target.to_path_buf(),
        backup,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "XRU1 ヘッダが切り詰められています".to_string())?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "XRU1 ヘッダが切り詰められています".to_string())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_f32(bytes: &[u8], offset: usize) -> Result<f32, String> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "XRU1 ヘッダが切り詰められています".to_string())?;
    Ok(f32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::env::Env;

    fn build_xru(kind: u8, hidden: u16, channels: u16, layer_shapes: &[(u32, u32)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"XRU1");
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.push(kind);
        bytes.push(0);
        bytes.extend_from_slice(&28u16.to_le_bytes());
        bytes.extend_from_slice(&hidden.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&(layer_shapes.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&0.15f32.to_le_bytes());
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.resize(64, 0);
        for &(input, output) in layer_shapes {
            bytes.extend_from_slice(&input.to_le_bytes());
            bytes.extend_from_slice(&output.to_le_bytes());
            bytes.extend_from_slice(&(1.0f32 / 127.0).to_le_bytes());
            bytes.extend_from_slice(&64.0f32.to_le_bytes());
            bytes.extend_from_slice(&1.0f32.to_le_bytes());
        }
        for &(input, output) in layer_shapes {
            bytes.extend(std::iter::repeat_n(0u8, (input * output) as usize));
            for _ in 0..output {
                bytes.extend_from_slice(&0i32.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn accepts_valid_mlp_cnn_and_offset_models() {
        let mlp = build_xru(0, 16, 0, &[(28, 16), (16, 1)]);
        assert_eq!(
            validate_header(&mlp, mlp.len() as u64).unwrap().kind,
            ModelKind::Mlp
        );
        let cnn = build_xru(1, 0, 16, &[(28, 16), (16, 16), (16, 3)]);
        assert_eq!(
            validate_header(&cnn, cnn.len() as u64).unwrap().kind,
            ModelKind::ResidualCnn
        );
        let srpo = build_xru(2, 0, 16, &[(28, 16), (16, 16), (16, 3)]);
        assert_eq!(
            validate_header(&srpo, srpo.len() as u64).unwrap().kind,
            ModelKind::Offset
        );
    }

    #[test]
    fn rejects_bad_magic_version_size_and_shapes() {
        let mut bytes = build_xru(1, 0, 16, &[(28, 16), (16, 16), (16, 3)]);
        bytes[0] = b'Y';
        assert!(validate_header(&bytes, bytes.len() as u64).is_err());

        let mut bytes = build_xru(1, 0, 16, &[(28, 16), (16, 16), (16, 3)]);
        bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
        assert!(validate_header(&bytes, bytes.len() as u64).is_err());

        let bytes = build_xru(1, 0, 16, &[(28, 16), (16, 16), (16, 3)]);
        assert!(validate_header(&bytes, bytes.len() as u64 + 1).is_err());

        let bytes = build_xru(1, 0, 16, &[(28, 16), (16, 8), (16, 3)]);
        assert!(validate_header(&bytes, bytes.len() as u64).is_err());
    }

    #[test]
    fn installs_model_with_backup_and_rejects_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.xru");
        let target = dir.path().join("target.xru");
        let bytes = build_xru(0, 16, 0, &[(28, 16), (16, 1)]);
        std::fs::write(&source, &bytes).unwrap();
        let first = install(&source, &target).unwrap();
        assert!(target.exists());
        assert!(first.backup.is_none());
        std::fs::write(&source, &bytes).unwrap();
        let second = install(&source, &target).unwrap();
        assert!(second.backup.unwrap().exists());
        assert!(install(&target, &target).is_err());
    }

    #[test]
    fn default_model_path_uses_home_config() {
        let env = Env {
            home: Some("/home/u".into()),
            ..Default::default()
        };
        assert_eq!(
            default_model_path(&env),
            std::path::PathBuf::from("/home/u/.config/xrizer/upscaler_model.xru")
        );
        assert_eq!(
            default_model_path(&Env::default()),
            std::path::PathBuf::from("/tmp/xrizer_upscaler_model.xru")
        );
    }
}
