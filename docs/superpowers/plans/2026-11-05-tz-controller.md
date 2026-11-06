# tz-controller Implementation Plan

> **Historical note (rename C1, 2026-11-06):** この文書は旧名 `XRizer-tz Config` / `xrizer-tz-config` 時代の設計・実装記録です。現在の名前は `tz-controller` です。本文は現在の名前・パスに更新しています。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** XRizer-tz-Custom の `upscaler.json`、`xrizer_scale`、`.xru` モデル、Vulkan レイヤー、Steam 起動オプションを安全に扱う egui デスクトップアプリを作る。

**Architecture:** 単一 Rust クレート。`src/core/` にファイル I/O・パス解決・検証・生成を集約し、`src/ui/` は egui の表示と操作だけを担当する。設定とスケールは明示保存、原子的書き込み + `.bak`、壊れた JSON は上書きしない。スケールは実効パス 1 本だけに書く。

**Tech Stack:** Rust 2024, eframe/egui 0.37 (glow, X11/Wayland, system_fonts), serde/serde_json (preserve_order), rfd 0.17, tempfile 3.

**Spec:** `docs/superpowers/specs/2026-11-05-tz-controller-design.md`

## Global Constraints

- 対象は Linux のみ。X11 / Wayland の両方を有効にする。
- 単一クレート。`src/core/` は egui に依存しない。
- ライセンスは GPL-3.0-or-later。同梱スクリプト / manifest の出所を `assets/README.md` に記録する。
- 設定とスケールは明示保存のみ。自動保存しない（GUI 自身の `settings.json` は除く）。
- 設定・スケール・モデルの書き込みは原子的に行い、設定 / スケール / モデルは `.bak` を 1 世代残す。
- 壊れた `upscaler.json` は絶対に上書きしない。未知キーは値と順序を保持する。
- スケールは実効パス 1 本だけに書く。新規作成時は永続候補（`~/.config` / `~/.local/share`）を揮発候補より優先する。
- 数値範囲・既定値・パス優先順位は XRizer `main` `e6fbdd4` の実装に一致させる。
- ユーザー向け文字列は日本語。コードの識別子とコメントは英語。
- テストは `tempfile::tempdir()` と合成 `Env` を使い、実ユーザーの `$HOME` / `$XDG_*` を変更しない。
- 各タスクの完了時に `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` を通す。
- コミットは Conventional Commits（例: `feat(config): preserve unknown keys`）。

## Review Focus

仕様が明示していないが、実利用で壊れると困る入力・失敗モード。各項目のテストを所有タスクに置く。

1. **未知キー入りの既存設定** — 保存後も未知キーの値と順序が残り、JSON が再読込できる（Task 3）。
2. **最優先の設定ファイルが壊れている / 型が違う** — 下位ファイルへ切り替えず、上書きせず、`Broken` として警告する（Task 3, Task 10）。
3. **複数候補・範囲外・パーセント・NaN / Infinity のスケール値** — 本体と同じ正規化と実効パス選択を行う（Task 4）。
4. **空白・引用符・`$`・改行・`&` / `|` を含むパス** — 起動オプションはシェル安全にクォートまたは拒否し、レイヤー導入は `$XDG_DATA_HOME` の sed 危険文字を拒否して manifest を検証する（Task 6, Task 7）。
5. **編集中の外部変更（設定 / スケール / `auto_scale`）** — 未編集なら自動再読込、編集中なら競合バナーを出し、保存直前にも再確認する（Task 11, Task 16）。

---

## Task 1: Repository bootstrap and GitHub repository

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `LICENSE`
- Create: `README.md`
- Create: `src/main.rs`
- Create: `src/app.rs`
- Create: `src/core/mod.rs`
- Create: `src/ui/mod.rs`
- Existing: `docs/superpowers/specs/2026-11-05-tz-controller-design.md`
- Existing: `docs/superpowers/plans/2026-11-05-tz-controller.md`

**Interfaces:**
- Produces: `App::new(cc: &eframe::CreationContext<'_>) -> App`, `impl eframe::App for App`.
- Produces: a compiling window titled `tz-controller`.

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "tz-controller"
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
license = "GPL-3.0-or-later"
description = "Configuration GUI for the XRizer-tz-Custom spatial upscaler"
repository = "https://github.com/TommyZ-7/tz-controller"

[dependencies]
eframe = { version = "0.37", default-features = false, features = [
    "clipboard",
    "default_fonts",
    "glow",
    "system_fonts",
    "wayland",
    "x11",
] }
rfd = "0.17"
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
tempfile = "3"

[profile.release]
lto = "thin"
strip = true
```

- [ ] **Step 2: Create `.gitignore`**

```gitignore
/target
*.swp
*~
.DS_Store
```

- [ ] **Step 3: Copy the GPL-3.0 license and add a placeholder README**

```bash
cp /home/tommyz7/work-saeki/xrizer/LICENSE LICENSE
```

`README.md` placeholder:

```markdown
# tz-controller

XRizer-tz-Custom の空間アップスケーラ設定 GUI。

License: GPL-3.0-or-later
```

Do not edit the GPL text.

- [ ] **Step 4: Create a minimal `src/main.rs`**

Use the eframe 0.37 API: `App::ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`, not `update(ctx, frame)`.

```rust
mod app;
mod core;
mod ui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tz-controller",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
```

- [ ] **Step 5: Create stub modules**

`src/core/mod.rs`:

```rust
pub mod atomic;
pub mod config;
pub mod env;
pub mod layer;
pub mod launch;
pub mod model;
pub mod scale;
pub mod settings;
```

For this task, create empty files for each module, or comment out the `pub mod` lines until the tasks add them. Prefer empty files with a module doc comment.

`src/ui/mod.rs`:

```rust
pub mod theme;
pub mod widgets;
```

`src/app.rs`:

```rust
pub struct App;

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("tz-controller");
            ui.label("準備中");
        });
    }
}
```

`src/ui/theme.rs` and `src/ui/widgets.rs` can be empty for now.

- [ ] **Step 6: Verify it compiles and runs**

Run: `cargo check`
Expected: success.

Run: `cargo run` (if a display is available)
Expected: a window titled `tz-controller` with the heading.

- [ ] **Step 7: Configure git identity and create the first commit**

```bash
git config user.name "TommyZ-7"
git config user.email "53462541+TommyZ-7@users.noreply.github.com"
git branch -M main
git add Cargo.toml Cargo.lock .gitignore LICENSE README.md src docs
git commit -m "chore: bootstrap tz-controller"
```

- [ ] **Step 8: Create the public GitHub repository and push**

```bash
gh repo create TommyZ-7/tz-controller --public --source=. --remote=origin --push
gh repo view TommyZ-7/tz-controller --json name,visibility,url
```

Expected: `"visibility":"PUBLIC"` and the repository URL.

- [ ] **Step 9: Commit the implementation plan**

The plan file is already in `docs/superpowers/plans/`. If it was not included in Step 7, commit it now:

```bash
git add docs/superpowers/plans/2026-11-05-tz-controller.md
git commit -m "docs: add implementation plan"
git push
```

---

## Task 2: Core environment and atomic file writes

**Files:**
- Create: `src/core/env.rs`
- Create: `src/core/atomic.rs`
- Modify: `src/core/mod.rs`
- Test: module tests inside both files

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub home: Option<std::path::PathBuf>,
    pub xdg_config_home: Option<std::path::PathBuf>,
    pub xdg_runtime_dir: Option<std::path::PathBuf>,
    pub xdg_data_home: Option<std::path::PathBuf>,
    pub xdg_cache_home: Option<std::path::PathBuf>,
    pub xrizer_upscaler_config: Option<std::path::PathBuf>,
    pub xrizer_scale_file: Option<std::path::PathBuf>,
}

impl Env {
    pub fn from_process() -> Self;
}

pub fn config_candidates(env: &Env) -> Vec<std::path::PathBuf>;
pub fn scale_candidates(env: &Env) -> Vec<std::path::PathBuf>;
pub fn data_home(env: &Env) -> Option<std::path::PathBuf>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupPolicy { None, BeforeReplace }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    pub path: std::path::PathBuf,
    pub backup: Option<std::path::PathBuf>,
}

pub fn write_atomic(
    path: &std::path::Path,
    bytes: &[u8],
    backup: BackupPolicy,
) -> Result<WriteOutcome, String>;

pub fn can_write_into(dir: &std::path::Path) -> bool;
```

- `Env::from_process()` reads `HOME`, `XDG_CONFIG_HOME`, `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XRIZER_UPSCALER_CONFIG`, `XRIZER_SCALE_FILE`. Empty values are treated as unset.
- `config_candidates` order: `XRIZER_UPSCALER_CONFIG`, `$XDG_CONFIG_HOME/xrizer/upscaler.json`, `$HOME/.config/xrizer/upscaler.json`, `$XDG_RUNTIME_DIR/xrizer_upscaler.json`, `/tmp/xrizer_upscaler.json`. Deduplicate preserving order.
- `scale_candidates` order: `XRIZER_SCALE_FILE`, `$XDG_RUNTIME_DIR/xrizer_scale`, `$HOME/.config/xrizer_scale`, `$HOME/.local/share/xrizer_scale`, `/tmp/xrizer_scale`. Deduplicate.
- `data_home`: `XDG_DATA_HOME`, else `$HOME/.local/share`, else `None`.
- `can_write_into(dir)`: find the nearest existing ancestor of `dir`; if it is a directory, create a `NamedTempFile` there and drop it. Returns `false` on any error.
- `write_atomic`:
  - Resolve symlinks: if `path` is a symlink, `canonicalize` it and write to the target; keep the symlink.
  - Refuse non-regular existing targets (directories, devices).
  - Create parent directories with `create_dir_all`.
  - Create a `NamedTempFile` in the target directory, write bytes, `flush`, `sync_all`.
  - Preserve the existing file's permissions; for a new file use `0o644` on Unix.
  - If `BackupPolicy::BeforeReplace` and the target exists, copy it to `<file_name>.bak` and `sync_all` the backup. If backup fails, return an error without replacing.
  - `persist` the temp file over the target.
  - Best-effort fsync the parent directory.
  - Return the target and backup path.

- [ ] **Step 1: Write failing tests for `env.rs`**

```rust
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
            "/home/u/.config/xrizer/upscaler.json".into(),
            "/run/user/1000/xrizer_upscaler.json".into(),
            "/tmp/xrizer_upscaler.json".into(),
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
            "/run/user/1000/xrizer_scale".into(),
            "/home/u/.config/xrizer_scale".into(),
            "/home/u/.local/share/xrizer_scale".into(),
            "/tmp/xrizer_scale".into(),
        ]
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test core::env -- --nocapture`
Expected: FAIL because `Env` / `config_candidates` do not exist.

- [ ] **Step 3: Implement `env.rs`**

Implement the types and functions above. Use `std::env::var_os` and filter empty `OsString`s.

- [ ] **Step 4: Write failing tests for `atomic.rs`**

```rust
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
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
}

#[test]
fn refuses_to_replace_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("target");
    std::fs::create_dir(&path).unwrap();
    assert!(write_atomic(&path, b"x", BackupPolicy::None).is_err());
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test core::atomic -- --nocapture`
Expected: FAIL because `write_atomic` does not exist.

- [ ] **Step 6: Implement `atomic.rs`**

Use `tempfile::NamedTempFile`. Use `std::os::unix::fs::PermissionsExt` for mode bits. Do not leave temp files behind on error; `NamedTempFile` handles drop cleanup. If `persist` fails, map the error to a string.

- [ ] **Step 7: Run tests**

Run: `cargo test core::env core::atomic -- --nocapture`
Expected: PASS.

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/core/mod.rs src/core/env.rs src/core/atomic.rs
git commit -m "feat(core): add path resolution and atomic writes"
```

---

## Task 3: Config core (`upscaler.json`)

**Files:**
- Create: `src/core/config.rs`
- Modify: `src/core/mod.rs`
- Test: module tests inside `src/core/config.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Fast,
    #[default]
    Balanced,
    Fsr,
    Ml,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
    pub model_path: Option<std::path::PathBuf>,
    pub auto_scale: bool,
    pub min_scale: f32,
    pub max_scale: f32,
}

impl Default for UpscalerConfig { /* values from the spec */ }

pub const KNOWN_KEYS: [&str; 14];

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: std::path::PathBuf,
    pub raw: serde_json::Map<String, serde_json::Value>,
    pub value: UpscalerConfig,
    pub notices: Vec<String>,
    pub existed: bool,
}

#[derive(Debug, Clone)]
pub struct BrokenConfig {
    pub path: std::path::PathBuf,
    pub error: String,
    pub raw_text: String,
}

#[derive(Debug, Clone)]
pub struct ConfigPathInfo {
    pub candidates: Vec<std::path::PathBuf>,
    pub path: std::path::PathBuf,
    pub existed: bool,
}

pub fn resolve_path(env: &crate::core::env::Env) -> Result<ConfigPathInfo, String>;
pub fn load(path: &std::path::Path) -> Result<LoadedConfig, BrokenConfig>;
pub fn sanitize(value: UpscalerConfig) -> (UpscalerConfig, Vec<String>);
pub fn normalize_model_path(
    raw: Option<&str>,
    home: Option<&std::path::Path>,
) -> Result<Option<std::path::PathBuf>, String>;
pub fn build_json(
    raw: &serde_json::Map<String, serde_json::Value>,
    value: &UpscalerConfig,
) -> Result<String, String>;
pub fn save(
    path: &std::path::Path,
    raw: &serde_json::Map<String, serde_json::Value>,
    value: &UpscalerConfig,
) -> Result<crate::core::atomic::WriteOutcome, String>;
pub fn file_fingerprint(path: &std::path::Path) -> Option<u64>;
```

- Defaults: `enabled=false`, `preset=Balanced`, `center=[0.5,0.5]`, `radius=0.35`, `feather=0.10`, `strength=1.0`, `output_scale=1.0`, `preserve_alpha=true`, `compare=false`, `show_region=false`, `model_path=None`, `auto_scale=false`, `min_scale=0.5`, `max_scale=1.5`.
- `KNOWN_KEYS`: `enabled`, `preset`, `center`, `radius`, `feather`, `strength`, `output_scale`, `preserve_alpha`, `compare`, `show_region`, `model_path`, `auto_scale`, `min_scale`, `max_scale`.
- `resolve_path`: first candidate whose `exists()` is true (do not fall through on parse failure); if none, first candidate whose parent is writable via `can_write_into`. Return `Err` if no candidate is writable.
- `load`: if `NotFound`, return `LoadedConfig` with empty raw map, default value, `existed=false`. Other read errors, invalid UTF-8, invalid JSON, non-object top level, or known-key type errors return `BrokenConfig`. On success, call `sanitize`, add notices. If `model_path` is a relative path, add a notice but do not fail.
- `sanitize`: clamp the documented ranges; swap `min_scale` / `max_scale` if reversed; add a notice for each change. Do not change `preset`. If a parsed float is non-finite (for example a huge JSON exponent), fall back to the field's default and add a notice, matching XRizer's `sanitize_finite`.
- `normalize_model_path`: `None` / empty / whitespace -> `None`; leading `~` or `~/` -> `$HOME` expansion; `~user` -> error; relative path -> error; absolute -> `Some`.
- `build_json`: clone `raw`, serialize `value` to a JSON object, insert all known keys. Use 2-space pretty printing and a trailing newline.
- `save`: `build_json`, verify by `serde_json::from_str::<UpscalerConfig>` round-trip, then `write_atomic(path, ..., BackupPolicy::BeforeReplace)`.
- `file_fingerprint`: read bytes and hash with `std::collections::hash_map::DefaultHasher`; `None` on missing/unreadable.

- [ ] **Step 1: Write failing tests**

```rust
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
    assert_eq!(value["radius"], 0.8);
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
    assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"enabled": true, "#);
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
    let env = crate::core::env::Env {
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
    let env = crate::core::env::Env {
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
        Some("/home/u/m.xru".into())
    );
    assert_eq!(normalize_model_path(Some("   "), Some(home)).unwrap(), None);
    assert!(normalize_model_path(Some("models/m.xru"), Some(home)).is_err());
    assert!(normalize_model_path(Some("~other/m.xru"), Some(home)).is_err());
}
```

Add a save test:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test core::config -- --nocapture`
Expected: FAIL because the module is incomplete.

- [ ] **Step 3: Implement the config types and functions**

Use `serde_json::Map` with `preserve_order`. Use `serde_json::from_value` with a cloned object to detect type errors. For `sanitize`, compare floats with `f32::EPSILON` and push Japanese notices (user-visible, so Japanese is allowed even in core).

- [ ] **Step 4: Run tests**

Run: `cargo test core::config -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/core/config.rs src/core/mod.rs
git commit -m "feat(config): load and save upscaler.json safely"
```

---

## Task 4: Scale core (`xrizer_scale`)

**Files:**
- Create: `src/core/scale.rs`
- Modify: `src/core/mod.rs`
- Test: module tests inside `src/core/scale.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ScaleState {
    Valid { raw: f32, value: f32 },
    Invalid { error: String },
    Missing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScaleCandidate {
    pub path: std::path::PathBuf,
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
    pub effective: Option<std::path::PathBuf>,
    pub effective_value: Option<f32>,
    pub source: Option<EffectiveSource>,
    pub persistent_fallback: Option<(std::path::PathBuf, f32)>,
    pub notices: Vec<String>,
    pub fingerprint: u64,
}

#[derive(Debug, Clone)]
pub struct ScaleSaveOutcome {
    pub path: std::path::PathBuf,
    pub backup: Option<std::path::PathBuf>,
    pub previous_value: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScaleSaveError {
    Conflict { path: std::path::PathBuf, current_value: f32 },
    NoTarget { reason: String },
    Io(String),
}

pub fn snapshot(env: &crate::core::env::Env) -> ScaleSnapshot;
pub fn snapshot_from_candidates(
    candidates: &[std::path::PathBuf],
    env: &crate::core::env::Env,
) -> ScaleSnapshot;
pub fn normalize_value(raw: f32) -> (f32, Vec<String>);
pub fn parse_value(text: &str) -> Result<(f32, Vec<String>), String>;
pub fn format_value(value: f32) -> String;
pub fn is_persistent(path: &std::path::Path, env: &crate::core::env::Env) -> bool;
pub fn save(
    env: &crate::core::env::Env,
    value: f32,
    expected_fingerprint: Option<u64>,
) -> Result<ScaleSaveOutcome, ScaleSaveError>;
```

- `normalize_value`: if `raw > 4.0`, divide by 100; then clamp to `0.1..=4.0`; NaN -> `0.1`; `+Infinity` -> `4.0`; `-Infinity` -> `0.1`. Add notices when the value changed.
- `parse_value`: `trim().parse::<f32>()`, then `normalize_value`.
- `format_value`: `format!("{value:.3}")`, no newline.
- `is_persistent`: false if the path starts with `XDG_RUNTIME_DIR` or `/tmp`; true otherwise.
- `snapshot`:
  - Read each candidate. `Ok` + parse -> `Valid`; `NotFound` -> `Missing`; other read/parse error -> `Invalid`.
  - `writable`: existing files via `OpenOptions::new().write(true).open(path).is_ok()`; missing files via `can_write_into(parent)`.
  - If a `Valid` exists, `effective` = first valid, `source=Existing`, `effective_value=Some(value)`.
  - If no `Valid`, choose the first `Missing` + writable + persistent candidate; if none, the first `Missing` + writable candidate; if none, `source=None`, `effective=None`.
  - `persistent_fallback`: if the effective path is volatile, find the first later `Valid` persistent candidate and return its path/value.
  - `fingerprint`: hash every candidate path, state discriminator, and raw value/error text with `DefaultHasher`.
- `save`:
  - Normalize the value (clamp; notices are not returned here, the UI already clamps).
  - Recompute `snapshot`.
  - If `expected_fingerprint` is `Some` and differs from the new snapshot's fingerprint, return `Conflict` with the new effective path/value.
  - Determine target: existing effective path, or the new-file target selected by the snapshot.
  - If no target, return `NoTarget`.
  - Write `format_value(value)` with `write_atomic(..., BeforeReplace)`.
  - Verify by parsing the file again.
  - Return the outcome with the previous value.

- [ ] **Step 1: Write failing tests**

```rust
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
    assert_eq!(snap.effective.unwrap(), dir.path().join(".config/xrizer_scale"));
    assert_eq!(snap.effective_value, Some(0.7));
    assert!(matches!(snap.candidates[0].state, ScaleState::Invalid { .. }));
    assert_eq!(snap.candidates[1].state, ScaleState::Valid { raw: 0.7, value: 0.7 });
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
    let snap = snapshot(&env_with(dir.path(), &runtime));
    assert_eq!(snap.effective.unwrap(), dir.path().join(".config/xrizer_scale"));
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
```

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test core::scale -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement `scale.rs`**

Implement the types and functions. Use `std::fs::read_to_string` and `parse_value`. Do not create directories during `snapshot`. Use `crate::core::atomic::can_write_into` and `write_atomic`.

- [ ] **Step 4: Run tests**

Run: `cargo test core::scale -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Add the volatile-fallback test**

```rust
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
```

Run: `cargo test core::scale -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/core/scale.rs src/core/mod.rs
git commit -m "feat(scale): resolve the effective xrizer_scale path"
```

---

## Task 5: Model core (XRU1 validation and placement)

**Files:**
- Create: `src/core/model.rs`
- Modify: `src/core/mod.rs`
- Test: module tests inside `src/core/model.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Mlp,
    ResidualCnn,
    Offset,
}

impl ModelKind {
    pub fn label(self) -> &'static str;
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
    pub target: std::path::PathBuf,
    pub backup: Option<std::path::PathBuf>,
}

pub const FEATURE_COUNT: u32 = 28;

pub fn default_model_path(env: &crate::core::env::Env) -> std::path::PathBuf;
pub fn inspect(path: &std::path::Path) -> Result<ModelInfo, String>;
pub fn validate_header(
    header_and_table: &[u8],
    file_size: u64,
) -> Result<ModelInfo, String>;
pub fn install(
    source: &std::path::Path,
    target: &std::path::Path,
) -> Result<ModelInstallOutcome, String>;
```

- `default_model_path`: `$HOME/.config/xrizer/upscaler_model.xru`; if `HOME` is absent, `/tmp/xrizer_upscaler_model.xru`.
- Header (little-endian):
  - `0..4` magic `XRU1`
  - `4..6` version u16 = 1
  - `6` kind u8 (0 MLP, 1 Residual CNN, 2 Offset)
  - `8..10` feature_count u16
  - `10..12` hidden u16
  - `12..14` channels u16
  - `14..16` layer_count u16
  - `16..20` input_scale f32
  - `20..24` activation_scale f32
  - `24..28` residual_clamp f32
  - `28..32` scale f32
  - `32..64` reserved
  - Layer entry 20 bytes: input u32, output u32, weight_scale f32, input_scale f32, output_scale f32.
- Validate:
  - length >= 64, magic, version 1, kind 0/1/2, layer_count > 0.
  - scalars finite, non-negative; `residual_clamp <= 1.0`.
  - layer table length = 64 + 20 × layer_count.
  - each layer input/output > 0, scales finite and > 0.
  - first layer input == feature_count; each adjacent output == next input.
  - MLP: feature_count == 28, hidden 8/16, exactly 2 layers, first output == hidden, second input == hidden, second output == 1.
  - CNN/Offset: feature_count == 28, channels 8/16/32, exactly 3 layers, shapes `(28, channels)`, `(channels, channels)`, `(channels, 3)`.
  - expected size = 64 + 20 × layer_count + Σ(input × output + output × 4); must equal `file_size`.
- `inspect`: metadata must be a regular file; read 64 bytes, parse layer_count, read the layer table, call `validate_header` with metadata.len().
- `install`:
  - `inspect(source)` must pass.
  - If `source` and `target` refer to the same file (canonicalize both if possible), return an error.
  - Create the target parent.
  - Copy source to a `NamedTempFile` in the target directory; `sync_all`.
  - `inspect` the temp file.
  - If target exists, copy to `<name>.bak`.
  - `persist` the temp file; fsync the directory best-effort.
  - Return target and backup.

- [ ] **Step 1: Write the test helper and failing tests**

```rust
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
        bytes.extend(std::iter::repeat_n(0i8, (input * output) as usize));
        for _ in 0..output {
            bytes.extend_from_slice(&0i32.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn accepts_valid_mlp_cnn_and_offset_models() {
    let mlp = build_xru(0, 16, 0, &[(28, 16), (16, 1)]);
    assert_eq!(validate_header(&mlp, mlp.len() as u64).unwrap().kind, ModelKind::Mlp);
    let cnn = build_xru(1, 0, 16, &[(28, 16), (16, 16), (16, 3)]);
    assert_eq!(validate_header(&cnn, cnn.len() as u64).unwrap().kind, ModelKind::ResidualCnn);
    let srpo = build_xru(2, 0, 16, &[(28, 16), (16, 16), (16, 3)]);
    assert_eq!(validate_header(&srpo, srpo.len() as u64).unwrap().kind, ModelKind::Offset);
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test core::model -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement `model.rs`**

Implement the header parser and validation. For reading the layer table, open the file, read 64 bytes, parse layer_count, then read `20 * layer_count` bytes. Keep memory bounded (max ~1.3 MB). Use `tempfile::NamedTempFile` for the atomic copy.

- [ ] **Step 4: Run tests**

Run: `cargo test core::model -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/core/model.rs src/core/mod.rs
git commit -m "feat(model): validate XRU1 models and install them safely"
```

---

## Task 6: Vulkan layer core and bundled installer

**Files:**
- Create: `assets/install-vulkan-layer.sh`
- Create: `assets/manifest.json.in`
- Create: `assets/README.md`
- Create: `src/core/layer.rs`
- Modify: `src/core/mod.rs`
- Test: module tests inside `src/core/layer.rs`, plus a shell integration test

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerPaths {
    pub data_home: std::path::PathBuf,
    pub manifest_dir: std::path::PathBuf,
    pub manifest: std::path::PathBuf,
    pub lib_dir: std::path::PathBuf,
    pub lib: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerStatus {
    Installed { library_path: std::path::PathBuf, lib_size: u64 },
    Partial { detail: String },
    NotInstalled,
    Error { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSnapshot {
    pub paths: Option<LayerPaths>,
    pub status: LayerStatus,
}

#[derive(Debug, Clone)]
pub struct SoInfo {
    pub path: std::path::PathBuf,
    pub size: u64,
    pub hash: u64,
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone)]
pub struct InstallResult {
    pub stdout: String,
    pub stderr: String,
    pub snapshot: LayerSnapshot,
}

pub fn layer_paths(env: &crate::core::env::Env) -> Result<LayerPaths, String>;
pub fn snapshot(env: &crate::core::env::Env) -> LayerSnapshot;
pub fn inspect_so(path: &std::path::Path) -> Result<SoInfo, String>;
pub fn auto_detect_so(env: &crate::core::env::Env) -> Option<std::path::PathBuf>;
pub fn hash_file(path: &std::path::Path) -> Result<u64, String>;
pub fn validate_data_home_for_sed(data_home: &std::path::Path) -> Result<(), String>;
pub fn install(
    env: &crate::core::env::Env,
    so_path: &std::path::Path,
) -> Result<InstallResult, String>;
pub fn remove(env: &crate::core::env::Env) -> Result<InstallResult, String>;
pub fn materialize_installer() -> Result<tempfile::TempDir, String>;
```

- Bundle the exact files from XRizer `main` `e6fbdd4`:
  - `cp /home/tommyz7/work-saeki/xrizer/scripts/install-vulkan-layer.sh assets/install-vulkan-layer.sh`
  - `cp /home/tommyz7/work-saeki/xrizer/vulkan-layer/manifest.json.in assets/manifest.json.in`
  - `sha256sum assets/install-vulkan-layer.sh assets/manifest.json.in`
- Expected hashes (from XRizer `main` `e6fbdd4`):
  - script: `9b1d10894dd4d261b4b1f34092815a58be831836207893d54d02646215ba3f60`
  - manifest: `07755c72b67a92081c12795246153e6907efd13ae59e8e04d254aa7f2a1342a8`
- Write `assets/README.md` with source URLs, commit `e6fbdd4`, the two sha256 values, and "GPL-3.0-or-later, from XRizer":

```markdown
# Bundled Vulkan layer installer

- Source: https://github.com/TommyZ-7/XRizer-tz-Custom
- Commit: e6fbdd4783b7082dbaa71eba094b817504ef6e97
- `install-vulkan-layer.sh` sha256: 9b1d10894dd4d261b4b1f34092815a58be831836207893d54d02646215ba3f60
- `manifest.json.in` sha256: 07755c72b67a92081c12795246153e6907efd13ae59e8e04d254aa7f2a1342a8
- License: GPL-3.0-or-later (same as XRizer-tz-Custom)
```
- Embed with `include_str!("../../assets/install-vulkan-layer.sh")` and `include_str!("../../assets/manifest.json.in")`.
- `materialize_installer`: create a `TempDir`, write `scripts/install-vulkan-layer.sh` (mode 0755) and `vulkan-layer/manifest.json.in` (mode 0644).
- `layer_paths`: `data_home` from `env::data_home`; manifest dir `$data_home/vulkan/implicit_layer.d`; manifest `xrizer_upscaler_layer.json`; lib dir `$data_home/xrizer/vulkan-layer`; lib `libxrizer_upscaler_layer.so`. If the resolved data home is relative, make it absolute against `std::env::current_dir()` or return an error; the manifest's `library_path` must be absolute.
- `snapshot`: if `layer_paths` fails, `paths=None`, `Error`. Otherwise:
  - manifest missing + lib missing -> `NotInstalled`.
  - manifest present but parse error -> `Error`.
  - manifest layer name != `VK_LAYER_XRIZER_upscaler` -> `Partial`.
  - resolve `library_path` relative to the manifest dir if relative; missing -> `Partial`.
  - manifest + lib present -> `Installed { library_path, lib_size }`.
  - manifest missing + lib present -> `Partial`.
- `inspect_so`: require absolute path, regular file, size > 0, first 4 bytes `\x7fELF`, no newline in the path string. Compute `hash_file`.
- `hash_file`: FNV-1a 64-bit streaming.
- `auto_detect_so`: `$HOME/work-saeki/xrizer/target/release/libxrizer_upscaler_layer.so`, then `target/debug/...`; return the first existing.
- `validate_data_home_for_sed`: reject if the path string contains any of `|`, `&`, `\`, `"`, `\n`, `\r`.
- `install`:
  - `layer_paths`, `validate_data_home_for_sed`, `inspect_so`.
  - Materialize the installer.
  - Run `bash <temp>/scripts/install-vulkan-layer.sh --user` with `XRIZER_LAYER_LIB=<absolute so path>`, `XDG_DATA_HOME=<data_home>`, and `HOME=<home>` if set.
  - Capture stdout/stderr. Non-zero exit -> error including both.
  - Read the installed manifest, parse `layer.name` and `library_path`; require the name and that the resolved library path equals `paths.lib` (canonicalize if possible).
  - If verification fails, run `--remove` for rollback, then return an error.
  - Return `InstallResult` with the new snapshot.
- `remove`: materialize, run `--remove` with the same env, capture output, return snapshot.

- [ ] **Step 1: Copy the assets and record hashes**

Run the `cp` and `sha256sum` commands above. Create `assets/README.md`.

- [ ] **Step 2: Write failing tests**

```rust
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
    assert_eq!(installed.snapshot.status, LayerStatus::Installed {
        library_path: data.join("xrizer/vulkan-layer/libxrizer_upscaler_layer.so"),
        lib_size: std::fs::metadata(&so).unwrap().len(),
    });
    assert!(data.join("vulkan/implicit_layer.d/xrizer_upscaler_layer.json").exists());
    let removed = remove(&env).unwrap();
    assert_eq!(removed.snapshot.status, LayerStatus::NotInstalled);
}
```

The `lib_size` in the installed snapshot is the installed file's size, which equals the source dummy size. Good.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test core::layer -- --nocapture`
Expected: FAIL.

- [ ] **Step 4: Implement `layer.rs`**

Use `std::process::Command`. Do not use a shell string; pass arguments separately. Set `XDG_DATA_HOME` and `HOME` explicitly. Use `tempfile::TempDir` for the installer. Verify the manifest after install.

- [ ] **Step 5: Run tests**

Run: `cargo test core::layer -- --nocapture`
Expected: PASS. The integration test requires `bash`, `sed`, `install`, and `rmdir`.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add assets src/core/layer.rs src/core/mod.rs
git commit -m "feat(layer): bundle and run the Vulkan layer installer"
```

---

## Task 7: Launch options core

**Files:**
- Create: `src/core/launch.rs`
- Modify: `src/core/mod.rs`
- Test: module tests inside `src/core/launch.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum LayerMode {
    #[default]
    Unspecified,
    Enable,
    Disable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum CaptureMode {
    #[default]
    Interval,
    Shutter,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

impl Default for LaunchForm { /* defaults from the spec */ }

impl LaunchForm {
    pub fn sanitize(&mut self) -> Vec<String>;
}

pub struct LaunchContext<'a> {
    pub env: &'a crate::core::env::Env,
    pub config_path: &'a std::path::Path,
    pub scale_path: Option<&'a std::path::Path>,
    pub layer_manifest_dir: Option<&'a std::path::Path>,
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

pub fn generate(form: &LaunchForm, ctx: &LaunchContext<'_>) -> LaunchOutput;
pub fn shell_quote(value: &str) -> Result<String, String>;
pub fn expand_tilde(value: &str, home: Option<&std::path::Path>) -> Result<String, String>;
```

- Defaults: `layer_mode=Unspecified`, `capture_enabled=false`, `capture_dir=""`, `capture_mode=Interval`, `interval_ms=500`, `max_pairs=300`, `scene=""`, `hold_ms=250`, `lockout_ms=1000`, pins false.
- `sanitize`: clamp interval 100..=60000, max_pairs 1..=100000, hold 100..=500, lockout 200..=5000; trim the scene? Do not trim the capture dir (paths can have trailing spaces, though unusual). Return notices.
- Output order:
  1. `XRIZER_UPSCALER_CONFIG` (pin_config_path)
  2. `XRIZER_SCALE_FILE` (pin_scale_path)
  3. `VK_ADD_IMPLICIT_LAYER_PATH` (layer mode Enable + pin_layer_path)
  4. `XRIZER_UPSCALER_VK_LAYER=1` or `XRIZER_UPSCALER_VK_LAYER_DISABLE=1`
  5. `XRIZER_CAPTURE=1`
  6. `XRIZER_CAPTURE_DIR`
  7. `XRIZER_CAPTURE_MODE`
  8. interval or hold/lockout
  9. `XRIZER_CAPTURE_MAX_PAIRS`
  10. `XRIZER_CAPTURE_SCENE` (non-empty)
  11. `%command%`
- `shell_quote`: safe characters `[A-Za-z0-9_./:@%+=,-]` unquoted; otherwise single-quote and replace `'` with `'\''`; reject NUL / newline / carriage return.
- `expand_tilde`: exact `~` -> home; `~/...` -> home/rest; other `~user` -> error; no tilde -> unchanged; home missing with tilde -> error.
- Validation:
  - capture dir empty -> error.
  - after `expand_tilde`, must be absolute; otherwise error.
  - NUL/newline -> error.
  - If errors are non-empty, return `command=""`, `env_only=""`, with the errors and any warnings.
- Warnings:
  - layer Enable && !layer_installed.
  - layer Enable && pin_layer_path && manifest dir missing.
  - pin_scale_path && scale path None.
  - preset_is_ml && layer_mode != Enable.
- `env_only` joins `KEY=VALUE` with spaces. `command` appends ` %command%` or is just `%command%`.

- [ ] **Step 1: Write failing tests**

```rust
fn ctx<'a>(env: &'a Env, config: &'a std::path::Path) -> LaunchContext<'a> {
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
    let config = std::path::Path::new("/home/u/.config/xrizer/upscaler.json");
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
    let env = Env { home: Some("/home/u".into()), ..Default::default() };
    let config = std::path::Path::new("/home/u/.config/xrizer/upscaler.json");
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
    let config = std::path::Path::new("/tmp/upscaler.json");
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
    let config = std::path::Path::new("/home/u/.config/xrizer/upscaler.json");
    let scale = std::path::Path::new("/run/user/1000/xrizer_scale");
    let manifest = std::path::Path::new("/home/u/.local/share/vulkan/implicit_layer.d");
    let form = LaunchForm {
        layer_mode: LayerMode::Enable,
        pin_config_path: true,
        pin_scale_path: true,
        pin_layer_path: true,
        ..Default::default()
    };
    let out = generate(&form, &LaunchContext {
        env: &env,
        config_path: config,
        scale_path: Some(scale),
        layer_manifest_dir: Some(manifest),
        layer_installed: true,
        preset_is_ml: true,
    });
    assert_eq!(
        out.command,
        "XRIZER_UPSCALER_CONFIG=/home/u/.config/xrizer/upscaler.json XRIZER_SCALE_FILE=/run/user/1000/xrizer_scale VK_ADD_IMPLICIT_LAYER_PATH=/home/u/.local/share/vulkan/implicit_layer.d XRIZER_UPSCALER_VK_LAYER=1 %command%"
    );
}

#[test]
fn rejects_relative_and_newline_capture_dirs() {
    let env = Env::default();
    let config = std::path::Path::new("/tmp/upscaler.json");
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test core::launch -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement `launch.rs`**

Implement the form, default, sanitize, generate, quote, and tilde expansion. Use `Vec<(String, String)>` for the vars to keep the order explicit.

- [ ] **Step 4: Run tests**

Run: `cargo test core::launch -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/core/launch.rs src/core/mod.rs
git commit -m "feat(launch): generate safe Steam launch options"
```

---

## Task 8: GUI settings core

**Files:**
- Create: `src/core/settings.rs`
- Modify: `src/core/mod.rs`
- Test: module tests inside `src/core/settings.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub layer_so_path: Option<String>,
    pub last_model_dir: Option<String>,
    pub launch: crate::core::launch::LaunchForm,
    pub active_tab: Option<String>,
}

impl Default for AppSettings { /* all None / launch default */ }

pub fn path(env: &crate::core::env::Env) -> std::path::PathBuf;
pub fn load(env: &crate::core::env::Env) -> (AppSettings, Option<String>);
pub fn save(
    env: &crate::core::env::Env,
    settings: &AppSettings,
) -> Result<crate::core::atomic::WriteOutcome, String>;
```

- Path: `$XDG_CONFIG_HOME/tz-controller/settings.json`, else `$HOME/.config/tz-controller/settings.json`, else `/tmp/tz-controller/settings.json`.
- Load: missing -> defaults, no warning. Parse error -> defaults + warning string. Never fail the app.
- Save: `serde_json::to_string_pretty` + newline, `write_atomic(..., BackupPolicy::BeforeReplace)`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn settings_path_follows_xdg() {
    let env = Env {
        home: Some("/home/u".into()),
        xdg_config_home: Some("/home/u/.config".into()),
        ..Default::default()
    };
    assert_eq!(path(&env), "/home/u/.config/tz-controller/settings.json");
}

#[test]
fn roundtrips_settings() {
    let dir = tempfile::tempdir().unwrap();
    let env = Env { home: Some(dir.path().into()), ..Default::default() };
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
    let env = Env { home: Some(dir.path().into()), ..Default::default() };
    let p = path(&env);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "{not json").unwrap();
    let (loaded, warning) = load(&env);
    assert!(warning.is_some());
    assert_eq!(loaded, AppSettings::default());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test core::settings -- --nocapture`
Expected: FAIL.

- [ ] **Step 3: Implement `settings.rs`**

Use `Env` and `atomic::write_atomic`. Derive `PartialEq` for `AppSettings`; `LaunchForm` already derives it.

- [ ] **Step 4: Run tests**

Run: `cargo test core::settings -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/core/settings.rs src/core/mod.rs
git commit -m "feat(settings): persist GUI preferences"
```

---

## Task 9: App shell, theme, widgets, and state

**Files:**
- Modify: `src/main.rs`
- Modify: `src/app.rs`
- Create: `src/ui/theme.rs`
- Create: `src/ui/widgets.rs`
- Create: `src/ui/upscaler.rs`
- Create: `src/ui/scale.rs`
- Create: `src/ui/model.rs`
- Create: `src/ui/layer.rs`
- Create: `src/ui/launch.rs`
- Create: `src/ui/info.rs`
- Modify: `src/ui/mod.rs`

**Interfaces:**
- Produces:

```rust
pub enum Tab {
    Upscaler,
    Scale,
    Model,
    Layer,
    Launch,
    Info,
}

pub struct App {
    pub env: crate::core::env::Env,
    pub active_tab: Tab,
    pub config: ConfigView,
    pub scale: ScaleView,
    pub model: ModelView,
    pub layer: LayerView,
    pub settings: crate::core::settings::AppSettings,
    pub settings_dirty: bool,
    pub settings_warning: Option<String>,
    pub launch_output: crate::core::launch::LaunchOutput,
    pub toast: Option<Toast>,
    pub confirm: Option<ConfirmDialog>,
    pub last_poll: std::time::Instant,
    pub last_settings_save: std::time::Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self;
    pub fn save_all(&mut self);
    pub fn reload_all(&mut self);
    pub fn poll_files(&mut self);
    pub fn poll_background(&mut self);
    pub fn apply_confirm(&mut self, action: ConfirmAction);
    pub fn show_toast(&mut self, kind: ToastKind, text: impl Into<String>);
}

pub enum ConfigState {
    Loaded,
    Missing,
    Broken { error: String, raw_text: String },
}

pub struct ConfigView { /* fields listed below */ }
pub struct ScaleView { /* fields listed below */ }
pub struct ModelView { /* fields listed below */ }
pub struct LayerView { /* fields listed below */ }
```

- `ConfigView` fields:
  - `path: PathBuf`
  - `candidates: Vec<PathBuf>`
  - `state: ConfigState` (`Loaded`, `Missing`, `Broken { error, raw_text }`)
  - `form: UpscalerConfig`
  - `model_path_text: String`
  - `saved_form: UpscalerConfig`
  - `saved_model_path_text: String`
  - `raw: serde_json::Map<String, serde_json::Value>`
  - `notices: Vec<String>`
  - `dirty: bool`
  - `fingerprint: Option<u64>`
  - `conflict: bool`
  - `error: Option<String>`
  - `save_message: Option<String>`
- `ScaleView` fields:
  - `snapshot: ScaleSnapshot`
  - `form_value: f32`
  - `saved_value: f32`
  - `dirty: bool`
  - `loaded_fingerprint: u64`
  - `conflict: bool`
  - `error: Option<String>`
  - `save_message: Option<String>`
- `ModelView` fields:
  - `target: PathBuf`
  - `current: Option<Result<ModelInfo, String>>`
  - `source_text: String`
  - `source: Option<Result<ModelInfo, String>>`
  - `busy: Option<std::sync::mpsc::Receiver<Result<ModelInstallOutcome, String>>>`
  - `error: Option<String>`
  - `message: Option<String>`
- `LayerView` fields:
  - `so_text: String`
  - `so: Option<Result<SoInfo, String>>`
  - `snapshot: LayerSnapshot`
  - `busy: Option<LayerBusy>`
  - `log: String`
  - `error: Option<String>`
- `LayerBusy` enum: `Install(Receiver<Result<InstallResult, String>>)`, `Remove(Receiver<Result<InstallResult, String>>)`.
- `ToastKind`: `Info`, `Success`, `Error`.
- `ConfirmAction`: `ReloadAll`, `InstallModel { source, target }`, `RemoveLayer`, `OverwriteScale`, `OverwriteConfig`. Derive `Debug`, `Clone` so the dialog can take the action out before mutating the app.

- [ ] **Step 1: Implement `ui/theme.rs`**

```rust
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional(15.0),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::proportional(20.0),
        );
    });
}
```

Keep `NativeOptions::system_font_fallback` at its default `true`; eframe's `system_fonts` feature will load a Japanese font on demand.

- [ ] **Step 2: Implement `ui/widgets.rs`**

Functions:

```rust
pub fn section_title(ui: &mut egui::Ui, title: &str, help: &str);
pub fn status_badge(ui: &mut egui::Ui, text: &str, color: egui::Color32);
pub fn warning_banner(ui: &mut egui::Ui, text: &str);
pub fn error_banner(ui: &mut egui::Ui, text: &str);
pub fn help_label(ui: &mut egui::Ui, text: &str);
pub fn mono_path(ui: &mut egui::Ui, path: &std::path::Path);
pub fn toast(ui: &mut egui::Ui, text: &str, kind: ToastKind);
```

Use `egui::RichText` for colors and `ui.horizontal_wrapped` for banners. Do not depend on unstable frame/margin APIs.

- [ ] **Step 3: Implement the shell in `app.rs`**

`App::new`:
- `theme::apply(&cc.egui_ctx)`.
- `let env = Env::from_process();`
- `let (mut settings, settings_warning) = settings::load(&env);`
- If `settings.launch.capture_dir` is empty and `env.home` is Some, set it to `$HOME/xrizer-captures` and mark `settings_dirty=true`. Call `settings.launch.sanitize()` and append its notices to `settings_warning`; if it changed anything, mark `settings_dirty=true`.
- Resolve config path with `config::resolve_path`; load it. If resolution fails, create a `Broken` view with the error.
- Build `ScaleView` from `scale::snapshot(&env)`.
- Build `ModelView` with `target = model::default_model_path(&env)`, `current = if target.exists() { Some(model::inspect(&target)) } else { None }`.
- Build `LayerView` with `so_text` from settings or `auto_detect_so`, inspect it, snapshot.
- Implement constructors `ConfigView::new(&Env)`, `ScaleView::new(&Env)`, `ModelView::new(&Env)`, and `LayerView::new(&Env)` for this initial load. Tasks 10-13 add the save/refresh methods and can reuse the same logic.
- Use `settings.active_tab` to choose the initial tab. Map `"upscaler"`, `"scale"`, `"model"`, `"layer"`, `"launch"`, `"info"`; default to `Upscaler`.
- Store `settings_warning` in `App` for the Info tab.

`App::ui`:
- Call `self.poll_files()` when `last_poll.elapsed() >= Duration::from_secs(1)`, and call `self.poll_background()` every frame. Task 16 fills in the bodies; Task 9 can leave them empty.
- Add panels in this order so the central panel gets the remaining space: top, bottom, left, central.
- `egui::Panel::top("top").show(ui, |ui| { ... })` with the title, status badges, `再読み込み`, `未保存`, `保存`.
- `egui::Panel::bottom("status").show(ui, |ui| { ... })` with config/scale effective paths.
- `egui::Panel::left("nav").resizable(false).default_size(190.0).show(ui, |ui| { ... })` with tab buttons. Use `ui.selectable_label` or `ui.add_sized`.
- `egui::CentralPanel::default().show(ui, |ui| { egui::ScrollArea::vertical().show(ui, |ui| match self.active_tab { ... }) });`
- Render toast and confirm dialog after the panels.
- `ctx.request_repaint_after(Duration::from_secs(1))` for polling.
- Define `ConfirmDialog { title, body, confirm_label, action: ConfirmAction }`, `ConfirmAction`, `ToastKind`, and `Toast` in `app.rs`. `show_toast` keeps success/info toasts for 3 seconds; errors stay in the tab's persistent error banner even after the toast expires.
- Intermediate tasks may leave some enum variants or helpers unused. If `clippy -D warnings` complains, add a temporary `#[allow(dead_code)]` on that item and remove it in the task that starts using it (Task 16 at the latest). Render the dialog with `egui::Modal::new(...).show(ui.ctx(), ...)` or `egui::Window::new(...).show(ui.ctx(), ...)`. Implement `ReloadAll` here; Task 12 adds `InstallModel`, Task 13 adds `RemoveLayer`, and Task 16 adds the overwrite actions. Until then, a wildcard match may set `self.toast` to "未実装" so the app still compiles.
- Handle `Ctrl+S` / `F5`:
  ```rust
  let save = ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S));
  let reload = ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5));
  ```
  `consume_key` exists in egui 0.37. If not, use `i.key_pressed` with modifiers check.

- [ ] **Step 4: Add tab stubs**

Each `src/ui/<tab>.rs` has:

```rust
pub fn show(app: &mut crate::app::App, ui: &mut egui::Ui) {
    ui.heading("...");
    ui.label("準備中");
}
```

`ui/mod.rs` exports the modules.

- [ ] **Step 5: Verify compile**

Run: `cargo check`
Expected: success.

- [ ] **Step 6: Manual smoke test**

Run: `cargo run`
Expected: window with top bar, 6 sidebar items, bottom status bar; tabs switch. Japanese labels are visible (eframe system font fallback).

- [ ] **Step 7: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add src/app.rs src/main.rs src/ui
git commit -m "feat(ui): add app shell and navigation"
```

---

## Task 10: Upscaler tab and config save

**Files:**
- Modify: `src/ui/upscaler.rs`
- Modify: `src/app.rs`
- Test: no new core tests; manual checks

**Interfaces:**
- Consumes: `config::{UpscalerConfig, Preset, save, load, normalize_model_path}`.
- Produces: `impl ConfigView { pub fn reload(&mut self, env: &Env); pub fn save(&mut self, env: &Env, force: bool) -> bool; }`.
- Produces: `App::save_all` starts as a config-only save in this task; Task 11 extends it to the scale, and Task 16 adds conflict handling.

- [ ] **Step 1: Implement `ConfigView::reload` and `ConfigView::save`**

`reload`:
- `config::load(&self.path)`.
- On `Ok(loaded)`: set `state` to `Loaded`/`Missing`, `form=value`, `model_path_text` from `value.model_path` (lossy), `saved_form=value`, `saved_model_path_text`, `raw`, `notices`, `dirty=false`, `fingerprint=file_fingerprint(path)`, clear errors/conflict.
- On `Err(broken)`: `state=Broken { error, raw_text }`, keep the last form but disable editing; `dirty=false`.

`save(env, force)`:
- If `Broken`, set `error` and return false.
- Normalize `model_path_text` with `normalize_model_path(Some(&text), env.home.as_deref())`. On error, set `error` and return false.
- Build `value = form.clone(); value.model_path = normalized;`.
- If `!force`, check the current file fingerprint against `self.fingerprint`; if changed, set `conflict=true` and return false (the UI shows the overwrite confirmation).
- `config::save(&self.path, &self.raw, &value)`.
- On success, `reload`; set `save_message`; clear `conflict`; return true.
- On error, set `error`; return false.

`App::save_all` (config-only for this task):
- If `config.dirty` and the state is not `Broken`, call `config.save(&env, false)`. On success show a toast; on failure set `config.error`.
- If the config is broken, show "設定ファイルが壊れているため保存できません".
- If not dirty, show "変更はありません".

- [ ] **Step 2: Implement the upscaler UI**

- Top: `ui.horizontal` with a large `enabled` checkbox and status badges.
- Broken banner: if `ConfigState::Broken`, show the error, the path, a read-only raw preview (first 64 KiB), `再読み込み`, `エディタで開く` (`xdg-open`).
- Missing banner: "設定ファイルはまだありません。保存すると作成します。"
- Notices: show each sanitize notice in a warning banner.
- Preset cards: 4 `ui.selectable_value` in a horizontal group, with a description label below each. Use `ui.vertical` inside a `ui.group`.
- Weight field:
  - `center[0]`, `center[1]`, `radius`, `feather`, `strength` sliders with exact ranges.
  - `中央に戻す` button.
  - Weight preview: allocate 160x160, paint background, crosshair, circle at `radius`, outer circle at `radius + feather`. Use `painter.circle_stroke` and `painter.line_segment`.
- Output and flags: `output_scale` slider; checkboxes for `preserve_alpha`, `compare`, `show_region`.
- Auto scale: checkbox + min/max sliders; show the swap/normalization note.
- `model_path` text field + `クリア`; show the active model path (or "既定") and a warning if it is not the default.
- `このタブを既定値に戻す` button: `self.config.form = UpscalerConfig::default(); self.config.model_path_text.clear(); self.recompute_dirty();`.
- Unknown keys: `CollapsingHeader` with count and a read-only pretty JSON of the unknown map.
- At the bottom, `設定を保存` button calls `app.save_all()` (or `config.save` + toast).

- [ ] **Step 3: Wire dirty state**

Add `ConfigView::recompute_dirty`:

```rust
pub fn recompute_dirty(&mut self) {
    self.dirty = self.form != self.saved_form
        || self.model_path_text != self.saved_model_path_text;
}
```

Call it after every widget group (or at the end of the tab) if any response changed. A simple approach: compare at the end of the tab rendering each frame; no need to track individual responses.

- [ ] **Step 4: Verify compile and manual checks**

Run: `cargo check`
Expected: success.

Manual test with a temp HOME:

```bash
mkdir -p /tmp/xrizer-gui-test/config
HOME=/tmp/xrizer-gui-test XDG_CONFIG_HOME=/tmp/xrizer-gui-test/config cargo run
```

- Confirm all 14 fields are editable.
- Save and inspect `upscaler.json`.
- Add an unknown key to the file, reload, edit, save, confirm the key remains.

- [ ] **Step 5: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/ui/upscaler.rs src/app.rs
git commit -m "feat(ui): edit all upscaler settings"
```

---

## Task 11: Scale tab and conflict handling

**Files:**
- Modify: `src/ui/scale.rs`
- Modify: `src/app.rs`
- Test: no new core tests; manual checks

**Interfaces:**
- Consumes: `scale::{snapshot, save, ScaleSaveError, format_value}`.
- Produces: `impl ScaleView { pub fn reload(&mut self, env: &Env); pub fn save(&mut self, env: &Env, force: bool) -> bool; }`.

- [ ] **Step 1: Implement `ScaleView::reload` and `ScaleView::save`**

`reload`:
- `snapshot(env)`.
- `form_value = snapshot.effective_value.unwrap_or(1.0)`.
- `saved_value = form_value`, `dirty=false`, `loaded_fingerprint=snapshot.fingerprint`, clear error/conflict.

`save(env, force)`:
- `let expected = if force { None } else { Some(self.loaded_fingerprint) };`
- `scale::save(env, self.form_value, expected)`.
- On success: reload; `save_message`; return true.
- On `Conflict { path, current_value }`: set `conflict=true`, `error=Some(format!("外部で {path} が {current_value} に変更されました"))`; return false.
- On `NoTarget` / `Io`: set `error`; return false.

Extend `App::save_all` to save the config first (if dirty), then the scale (if dirty). If the config save fails, still attempt the scale save so one broken section does not block the other. Show one combined toast: `設定: 保存 / 変更なし / エラー、スケール: ...`.

- [ ] **Step 2: Implement the scale UI**

- Big slider 0.1..=4.0 with 0.05 step, `DragValue` with 3 decimals, percentage label.
- Quick buttons: 50%, 70%, 85%, 100%, 150%.
- Effective path: `mono_path`, source label (`既存` / `新規作成` / `なし`), current value.
- Volatile warning if the path is under `$XDG_RUNTIME_DIR` or `/tmp`; show `persistent_fallback` as "ログアウト後は <path> の <value>".
- `auto_scale` warning: read `app.config.form.auto_scale`; if true, show "本体がこのファイルを自動調整します。手動保存は次の調整で上書きされる可能性があります".
- Candidate table: path, state, writable, persistent. Use `egui::Grid`.
- Buttons: `スケールを保存` (calls `app.save_all()`), `再読込`.
- If `self.conflict`, show a banner with `このまま保存` (calls `save(env, true)`) and `再読み込み` (calls `reload`).
- If `snapshot.source == None`, show an error "有効なスケールファイルがなく、既存ファイルはすべて不正です" and disable save.

- [ ] **Step 3: Verify compile and manual checks**

Run: `cargo check`.

Manual test:

```bash
mkdir -p /tmp/xrizer-gui-test/config /tmp/xrizer-gui-test/run
HOME=/tmp/xrizer-gui-test XDG_CONFIG_HOME=/tmp/xrizer-gui-test/config \
XDG_RUNTIME_DIR=/tmp/xrizer-gui-test/run cargo run
```

- Save 70% -> confirm the file content is `0.700` and the path is persistent (`~/.config/xrizer_scale`).
- Create a valid `run/xrizer_scale` externally, reload -> the effective path becomes the runtime file and the warning shows.
- Edit the value, change the file externally, save -> conflict banner; `このまま保存` overwrites.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/ui/scale.rs src/app.rs
git commit -m "feat(ui): edit xrizer_scale at its effective path"
```

---

## Task 12: Model tab

**Files:**
- Modify: `src/ui/model.rs`
- Modify: `src/app.rs`
- Test: no new core tests; manual checks

**Interfaces:**
- Consumes: `model::{default_model_path, inspect, install, ModelInfo, ModelInstallOutcome}`.
- Consumes: `config::{normalize_model_path}`.
- Produces: `impl ModelView { pub fn refresh_current(&mut self, path: &Path); pub fn refresh_source(&mut self); }`.

- [ ] **Step 1: Implement `ModelView` helpers**

`refresh_current(path)`:
- If `path.exists()`, `current=Some(inspect(path))`; else `None`.

`refresh_source`:
- If `source_text.trim().is_empty()`, `source=None`; else `source=Some(inspect(Path::new(&source_text)))`.

- [ ] **Step 2: Implement the model UI**

- Current model section:
  - Determine the effective model path: normalize `app.config.model_path_text`; if `Some`, use it; else `app.model.target`.
  - If the normalized path is invalid, show an error.
  - If the path exists, show `ModelInfo` (kind, feature_count, hidden/channels, layer shapes, size, mtime).
  - If missing, show "モデルがありません。ml は FSR にフォールバックします。"
  - If invalid, show the error.
- Source selection:
  - `TextEdit::singleline` + `参照...` button using `rfd::FileDialog::new().add_filter("XRU model", &["xru"]).set_directory(last_model_dir).pick_file()`.
  - On change, `refresh_source`.
  - Show info/error for the source.
- `既定パスへ配置` button:
  - Disabled if the source is invalid, the source is the target, or a background task is running.
  - If the target exists, show a confirmation dialog (`ConfirmAction::InstallModel { source, target }`); Step 3 handles the confirmed action.
  - If the target does not exist, start the thread immediately.
  - The thread sends `model::install(&source, &target)` over an `mpsc::Receiver<Result<ModelInstallOutcome, String>>`.
  - Poll in `App::poll_background`; on success, refresh current, set `settings.last_model_dir`, set `settings_dirty=true`, show a success toast; on error, set `model.error`.
- `model_path` section:
  - Show the normalized path or "既定パス".
  - If it points elsewhere, warning + `model_path を null に戻す` button (sets `config.model_path_text.clear()` and `config.recompute_dirty()`).
- If the preset is not `ml`, show a note that the model is not used.

- [ ] **Step 3: Add the `InstallModel` confirmation arm**

In `App::apply_confirm`, add `ConfirmAction::InstallModel { source, target }`:
- Spawn the same thread described above and set `self.model.busy`.
- Clear `self.confirm`.

- [ ] **Step 4: Add the background poll**

In `App::poll_background`, if `self.model.busy` is Some:
- `try_recv`.
- On `Ok(result)`: `busy=None`; on `Ok(outcome)` refresh current, set message; on `Err(error)` set error.
- Request repaint while busy.

- [ ] **Step 5: Verify compile and generate a test model**

Run: `cargo check`.

Generate a valid XRU fixture with the XRizer checkout:

```bash
mkdir -p /tmp/xrizer-gui-test
cd /home/tommyz7/work-saeki/xrizer
XRIZER_SELF_TEST_MODEL_OUT=/tmp/xrizer-gui-test/sample.xru \
XRIZER_SELF_TEST_MODEL_KIND=residual_cnn \
XRIZER_SELF_TEST_MODEL_CHANNELS=16 \
cargo test --lib write_self_test_model_file -- --ignored --nocapture
```

Manual test:
- Select `/tmp/xrizer-gui-test/sample.xru`.
- Confirm the model info is shown.
- Place it into `HOME=/tmp/xrizer-gui-test/.config/xrizer/upscaler_model.xru` (run the app with the temp HOME).
- Select a non-XRU file and confirm placement is disabled.
- Set `model_path` to another path and confirm the tab shows that model and warns.

- [ ] **Step 6: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/ui/model.rs src/app.rs
git commit -m "feat(ui): select and install XRU models"
```

---

## Task 13: Vulkan layer tab

**Files:**
- Modify: `src/ui/layer.rs`
- Modify: `src/app.rs`
- Test: no new core tests; manual checks

**Interfaces:**
- Consumes: `layer::{snapshot, inspect_so, auto_detect_so, install, remove, LayerStatus, SoInfo}`.
- Produces: `impl LayerView { pub fn refresh(&mut self, env: &Env); pub fn refresh_so(&mut self); }`.

- [ ] **Step 1: Implement `LayerView` helpers**

`refresh(env)`:
- `snapshot=layer::snapshot(env)`.

`refresh_so`:
- If `so_text.trim().is_empty()`, `so=None`; else `so=Some(inspect_so(Path::new(&so_text)))`.

- [ ] **Step 2: Implement the layer UI**

- Explanation: only needed for `ml`; `fast` / `balanced` / `fsr` do not need it; fallback to FSR.
- `.so` path:
  - `TextEdit::singleline` + `参照...` (`rfd::FileDialog::new().add_filter("Vulkan layer", &["so"]).pick_file()`).
  - `自動検出` button calls `auto_detect_so`.
  - Show `SoInfo`: size, modified, hash.
  - Show validation errors.
- Status:
  - `Installed`: green badge, manifest path, library path, size.
  - `Partial`: amber badge, detail.
  - `NotInstalled`: gray badge.
  - `Error`: red badge, detail.
  - If installed and the selected `.so` hash differs from the installed hash, show "選択中の .so と導入済みが異なります。再インストールしてください". Compute the installed hash with `layer::hash_file(paths.lib)` when the status is installed.
- Buttons:
  - `インストール / 更新`: validate, spawn `layer::install(env, so_path)` in a thread; store `LayerBusy::Install`.
  - `削除`: confirmation dialog (`ConfirmAction::RemoveLayer`); on confirm spawn `layer::remove`.
  - While busy: disable buttons, show spinner.
- Log: `CollapsingHeader` with stdout/stderr in a read-only monospace text box.
- Note: install status is file-based; verify loading with `VK_LOADER_DEBUG=layer`; restart the game after install/remove.
- Show the bundled installer source commit (`e6fbdd4`) and license.

- [ ] **Step 3: Add the `RemoveLayer` confirmation arm**

In `App::apply_confirm`, add `ConfirmAction::RemoveLayer`:
- Spawn a thread that calls `layer::remove(&env)` and sends the result over an `mpsc::Receiver<Result<InstallResult, String>>`.
- Set `self.layer.busy = Some(LayerBusy::Remove(rx))` and clear `self.confirm`.

- [ ] **Step 4: Add background poll**

In `App::poll_background`, poll `LayerBusy` receivers:
- On install success: set `layer.snapshot`, `layer.log` (stdout + stderr), clear busy, show success toast. If the status is not Installed, set `layer.error`.
- On install error: set `layer.error`, keep the log, clear busy.
- On remove success: set `layer.snapshot`, log, clear busy, toast.
- On remove error: set `layer.error`.

- [ ] **Step 5: Verify compile and manual checks**

Run: `cargo check`.

Manual test with a temp data home:

```bash
mkdir -p /tmp/xrizer-gui-test/data
HOME=/tmp/xrizer-gui-test XDG_DATA_HOME=/tmp/xrizer-gui-test/data cargo run
```

- Select `/home/tommyz7/work-saeki/xrizer/target/debug/libxrizer_upscaler_layer.so`.
- Install -> status becomes `導入済み`; inspect the manifest and lib under `/tmp/xrizer-gui-test/data`.
- Change the selected `.so` (e.g., copy it to a new path and modify a byte) -> freshness warning.
- Remove -> status becomes `未導入`.

- [ ] **Step 6: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/ui/layer.rs src/app.rs
git commit -m "feat(ui): manage the Vulkan layer install"
```

---

## Task 14: Launch options tab

**Files:**
- Modify: `src/ui/launch.rs`
- Modify: `src/app.rs`
- Test: no new core tests; manual checks

**Interfaces:**
- Consumes: `launch::{generate, LaunchContext, LaunchForm, LayerMode, CaptureMode}`.
- Produces: `fn build_context(app: &App) -> LaunchContext<'_>` in `app.rs` or `ui/launch.rs`.

- [ ] **Step 1: Implement the context builder**

`build_context(app)`:
- `config_path = &app.config.path`.
- `scale_path = app.scale.snapshot.effective.as_deref()`.
- `layer_manifest_dir = app.layer.snapshot.paths.as_ref().map(|p| p.manifest_dir.as_path())`.
- `layer_installed = matches!(app.layer.snapshot.status, LayerStatus::Installed { .. })`.
- `preset_is_ml = app.config.form.preset == Preset::Ml`.
- `env = &app.env`.

- [ ] **Step 2: Implement the launch UI**

- Note: these vars are read at game start; restart the game after changes.
- Layer mode: 3 radio values (指定しない / 有効 / 強制無効). Show warnings from `launch_output`.
- Capture:
  - `キャプチャを有効にする` checkbox.
  - Dir text + `参照...` folder picker (`rfd::FileDialog::new().pick_folder()`); `~` is expanded by `generate`; show the expanded path.
  - Mode radio interval/shutter.
  - Interval ms DragValue (100..=60000).
  - Shutter hold ms (100..=500) / lockout ms (200..=5000).
  - Max pairs (1..=100000).
  - Scene text (optional).
- Advanced:
  - `設定ファイルのパスを固定する` checkbox.
  - `スケールファイルのパスを固定する` checkbox, disabled if no effective scale path.
  - `レイヤーの探索パスを固定する` checkbox, disabled unless layer mode is Enable and the manifest dir exists; default ON when layer mode becomes Enable and the layer is installed.
- Generated output:
  - Compute `app.launch_output = generate(&app.settings.launch, &ctx)` each frame.
  - Show `errors` in red and `warnings` in amber.
  - `TextEdit::multiline` with the command, read-only, monospace, desired width infinity.
  - `コピー` button calls `ui.ctx().copy_text(app.launch_output.command.clone())` and shows a toast. Disabled if there are errors.
  - `環境変数だけコピー` copies `env_only`.
- On any change, `app.settings_dirty = true`.

- [ ] **Step 3: Verify compile and manual checks**

Run: `cargo check`.

Manual test:
- Enable capture with a dir containing a space, e.g. `/tmp/xrizer captures`.
- Confirm the output quotes it as `'/tmp/xrizer captures'`.
- Set a scene with a single quote, e.g. `scene's`, and confirm `'\''`.
- Enable layer + pin paths; confirm `VK_ADD_IMPLICIT_LAYER_PATH` appears.
- Copy and inspect the string.

- [ ] **Step 4: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/ui/launch.rs src/app.rs
git commit -m "feat(ui): generate Steam launch options"
```

---

## Task 15: Info tab

**Files:**
- Modify: `src/ui/info.rs`
- Modify: `src/app.rs`
- Test: no new core tests

**Interfaces:**
- Consumes: all resolved paths and `env`.
- Produces: read-only information view.

- [ ] **Step 1: Implement the info UI**

- App version from `env!("CARGO_PKG_VERSION")`, build profile (`debug_assertions`), license `GPL-3.0-or-later`.
- Paths:
  - Config: path, state, candidates.
  - Scale: effective, source, candidates.
  - Model: default path, effective model path.
  - Layer: manifest, library, status.
  - GUI settings: path.
- Environment: `HOME`, `XDG_CONFIG_HOME`, `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`, `XRIZER_UPSCALER_CONFIG`, `XRIZER_SCALE_FILE`. Note that the game may see different values.
- Bundled installer: source commit `e6fbdd4`, script sha256, manifest sha256. Hard-code the sha256 values from `assets/README.md`.
- Buttons:
  - `設定ファイルをエディタで開く` -> `xdg-open <config path>` (spawn, ignore failure).
  - `設定ディレクトリを開く` -> `xdg-open <parent>`.
  - `GUI 設定を開く` -> `xdg-open <settings path>`.
- Show the GUI settings warning if `App::new` captured one.

- [ ] **Step 2: Verify compile**

Run: `cargo check`.

- [ ] **Step 3: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
git add src/ui/info.rs src/app.rs
git commit -m "feat(ui): show resolved paths and diagnostics"
```

---

## Task 16: Polling, conflicts, and save orchestration

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/upscaler.rs`
- Modify: `src/ui/scale.rs`
- Modify: `src/ui/launch.rs`
- Test: no new core tests; manual checks

**Interfaces:**
- Produces: `App::poll_files`, `App::poll_background`, `App::save_all`, `App::reload_all`.
- Produces: `ConfirmAction` handling.

- [ ] **Step 1: Extend `App::save_all` with conflict handling**

Order:
1. If `config.state` is `Broken`: skip config, set a message.
2. If `config.dirty`: call `config.save(&env, false)`. If it returns false due to a conflict, show `ConfirmAction::OverwriteConfig`; still attempt the scale save.
3. If `scale.dirty`: call `scale.save(&env, false)`.
   - On `Conflict`, show `ConfirmAction::OverwriteScale`; keep the scale dirty.
4. If neither dirty: show "変更はありません".
5. On success, show a combined toast.

`ConfirmAction::OverwriteConfig`:
- On confirm, call `config.save(&env, true)` (skip the fingerprint check).

`ConfirmAction::OverwriteScale`:
- On confirm, call `scale.save(&env, true)`.

- [ ] **Step 2: Implement `App::poll_files`**

Every 1 second:
- Config:
  - `current = config::file_fingerprint(&config.path)`.
  - If `current != config.fingerprint`:
    - If `config.dirty`: `config.conflict = true`.
    - Else: `config.reload(&env)`; toast "設定ファイルの外部変更を読み込みました".
- Scale:
  - `snap = scale::snapshot(&env)`.
  - If `snap.fingerprint != scale.loaded_fingerprint`:
    - If `scale.dirty`: `scale.conflict = true`.
    - Else: update `scale.snapshot`, `form_value`, `saved_value`, `loaded_fingerprint`; toast.
- Layer: `layer.snapshot = layer::snapshot(&env)` every 2 seconds (or every poll; cheap).
- Model: every 2 seconds, recompute the effective model path and call `model.refresh_current(&path)`. The header/table read is small; no mtime cache is needed.
- Set `last_poll = Instant::now()`.

- [ ] **Step 3: Complete `App::poll_background`**

- Keep the model/layer receiver handling added in Tasks 12 and 13.
- While any receiver is Some, request repaint in 100ms.
- On success, update `settings` and set `settings_dirty=true` where relevant.
- Save GUI settings when `settings_dirty` and 1 second has passed since the last change, and on exit (`on_exit`).
- `on_exit`: call `settings::save(&env, &settings)` best-effort.

- [ ] **Step 4: Implement the reload flow**

- `reload_all`:
  - If `config.dirty || scale.dirty`, show `ConfirmAction::ReloadAll`.
  - On confirm, `config.reload`, `scale.reload`, clear conflicts, toast.
- `F5` calls `reload_all`; `Ctrl+S` calls `save_all`.

- [ ] **Step 5: Add conflict banners**

- Upscaler tab: if `config.conflict`, show a banner with:
  - `再読み込み（未保存の変更を破棄）` -> `config.reload`.
  - `このまま保存（外部変更を上書き）` -> `config.save(&env, true)`.
- Scale tab: if `scale.conflict`, same pattern with `scale.save(&env, true)`.
- The banners must not disappear until the user chooses or saves successfully.

- [ ] **Step 6: Complete the confirm dialog actions**

Extend the generic dialog from Task 9. Render title/body and `実行` / `キャンセル` buttons. On `実行`, take the action and execute it after the borrow ends. On `キャンセル`, clear `self.confirm`.
- Actions: `ReloadAll`, `InstallModel`, `RemoveLayer`, `OverwriteScale`, `OverwriteConfig`.
- `InstallModel` and `RemoveLayer` were added in Tasks 12 and 13; keep their behavior. `ReloadAll` calls `reload_all`; the overwrite actions call the corresponding `save(..., true)`.

- [ ] **Step 7: Verify compile and manual conflict tests**

Run: `cargo check`.

Manual test:
- Start the app with a temp HOME/XDG.
- Edit a config field but do not save. Modify the file externally with `jq` or an editor. Confirm the conflict banner appears and the form is not overwritten.
- Choose `このまま保存` and confirm the external content is replaced (with `.bak`).
- Choose `再読み込み` and confirm the form matches the external content.
- Repeat for the scale file, including `auto_scale` simulation (write a new value externally).

- [ ] **Step 8: Commit**

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
git add src/app.rs src/ui/upscaler.rs src/ui/scale.rs src/ui/launch.rs
git commit -m "feat(ui): handle external changes and save conflicts"
```

---

## Task 17: README, CI, final verification, and push

**Files:**
- Modify: `README.md`
- Create: `.github/workflows/ci.yml`
- Modify: `src/app.rs` (only if final fixes are needed)

**Interfaces:**
- Produces: a documented, CI-tested release build.

- [ ] **Step 1: Write the README**

Cover:
- What the app does.
- Requirements: Linux, XRizer-tz-Custom `main` (upscaler + capture), Vulkan for `ml`.
- Build:
  ```bash
  cargo build --release
  ./target/release/tz-controller
  ```
- Tabs and what they do.
- Path priority tables for `upscaler.json` and `xrizer_scale`.
- Safety: atomic writes, `.bak`, broken JSON never overwritten, unknown keys preserved, explicit save only.
- Scale: effective path only, persistent preference for new files, volatile warnings, `auto_scale` conflicts.
- Model: default path, XRU1 validation, custom `model_path` must be absolute.
- Vulkan layer: bundled installer from XRizer `e6fbdd4`, select a built `.so`, install to `$XDG_DATA_HOME`, remove, `XRIZER_UPSCALER_VK_LAYER=1`, `VK_ADD_IMPLICIT_LAYER_PATH` advanced option, restart the game.
- Launch options: copy/paste into Steam, `%command%` at the end, capture env vars read at startup.
- Limitations: file-based layer status, environment differences, `ml` fallback, no Windows/macOS.
- License and bundled asset provenance.

- [ ] **Step 2: Add CI**

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
  pull_request:
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Install system dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev libegl1-mesa-dev
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
      - run: cargo build --release
```

If the build fails on a missing package, add it to the apt list based on the compiler error.

- [ ] **Step 3: Run the full verification locally**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Expected: all pass. Record the test count.

- [ ] **Step 4: Manual verification checklist**

With a temporary HOME/XDG:

```bash
export TESTROOT=/tmp/xrizer-gui-test
mkdir -p "$TESTROOT/config" "$TESTROOT/run" "$TESTROOT/data" "$TESTROOT/home"
HOME="$TESTROOT/home" XDG_CONFIG_HOME="$TESTROOT/config" \
XDG_RUNTIME_DIR="$TESTROOT/run" XDG_DATA_HOME="$TESTROOT/data" \
./target/release/tz-controller
```

- All 6 tabs open.
- All 14 config fields edit and save; JSON re-parses.
- Unknown keys survive a save.
- Broken JSON shows a persistent warning and disables config save; scale still saves.
- Scale writes only the effective path; persistent preference for a new file.
- Model validation and placement work with the XRizer self-test model.
- Layer install/remove works with the debug `.so`; status updates.
- Launch options copy with correct quoting and `%command%`.
- Conflict banners appear for external changes.

- [ ] **Step 5: Final commit and push**

```bash
git add README.md .github/workflows/ci.yml
git commit -m "docs: document usage and add CI"
git push
```

- [ ] **Step 6: Report**

Report:
- Repository URL.
- Commit hash.
- Test results (`cargo test`, `cargo clippy`, `cargo build --release`).
- Manual checks performed and any not performed (e.g., no display available).
- Any remaining limitations or follow-up items.
