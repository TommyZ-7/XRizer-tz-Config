# Rename XRizer-tz Config to tz-controller Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the config app from `XRizer-tz Config` / `xrizer-tz-config` to the suite name `tz-controller` without changing the XRizer-owned settings contract or config paths.

**Architecture:** C1 is a rename-only change across Cargo metadata, the binary and window title, GUI preference storage, AppImage packaging, CI, scripts, assets, and current documentation. Behavioral compatibility is preserved by reading the old GUI settings directory and old font environment variable when the new ones are absent, while all writes go to the new names.

**Tech Stack:** Rust 2024, eframe/egui, Cargo, Bash, GitHub Actions, AppImage tooling.

**Scope:** No GitHub repository rename, no DXVK settings page (C2), no release publication. `upscaler.json`, `xrizer_scale`, `XRIZER_UPSCALER_CONFIG`, and the settings-v1 contract remain owned by XRizer and are not renamed.

---

### Task 1: Package, binary, window, and font fallback

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`
- Modify: `src/main.rs`, `src/app.rs`

- [ ] Change the Cargo package to `tz-controller`, describe it as a suite controller for XRizer AA plus future DXVK VRS, and point the repository at the future `TommyZ-7/tz-controller` URL.
- [ ] Change the window title and top-bar heading to `tz-controller`.
- [ ] Read the bundled font from `TZ_CONTROLLER_FONT` first, then `XRIZER_TZ_CONFIG_FONT`. Make the resolver pure enough to test without mutating process environment.
- [ ] Add tests for new-variable precedence, old-variable fallback, and missing/non-file values.

### Task 2: GUI settings path migration

**Files:**
- Modify: `src/core/settings.rs`

- [ ] Resolve the new settings path as `$XDG_CONFIG_HOME/tz-controller/settings.json`, else `$HOME/.config/tz-controller/settings.json`, else `/tmp/tz-controller/settings.json`.
- [ ] On load, if the new file is missing, read the legacy `xrizer-tz-config/settings.json` path. If the new file exists, it always wins, including when broken.
- [ ] Save only to the new path; do not delete the legacy file.
- [ ] Add tests for the XDG path, legacy fallback, new-file precedence, and broken legacy handling.

### Task 3: AppImage, assets, scripts, and CI

**Files:**
- Rename: `assets/appimage/io.github.tommyz7.XRizerTzConfig.desktop` → `io.github.tommyz7.TzController.desktop`
- Rename: `assets/appimage/xrizer-tz-config.png` → `tz-controller.png`
- Rename: `assets/appimage/xrizer-tz-config.svg` → `tz-controller.svg`
- Modify: `assets/appimage/AppRun`, `scripts/build-appimage.sh`, `scripts/github-release.sh`, `scripts/test-github-release.sh`, `.github/workflows/appimage.yml`

- [ ] Use `tz-controller-<version>-x86_64.AppImage`, binary `tz-controller`, desktop ID `io.github.tommyz7.TzController`, and matching desktop/icon/doc paths.
- [ ] Update AppRun to export `TZ_CONTROLLER_FONT`, honoring the old `XRIZER_TZ_CONFIG_FONT` when already set, and exec `usr/bin/tz-controller`.
- [ ] Update the release title, CI window lookups, AppImage content checks, and artifact name `tz-controller-appimage`.
- [ ] Extend the release-helper test to assert the new title.

### Task 4: Documentation and historical notes

**Files:**
- Modify: `README.md`
- Rename/move: historical design and implementation docs to `2026-11-05-tz-controller-design.md` and `2026-11-05-tz-controller.md`
- Modify: historical plan/spec headers with a note that the old name is historical.

- [ ] Update README title, suite description, clone/build/AppImage commands, screenshot alt text, and doc links.
- [ ] Document the old GitHub URL redirect, legacy settings read path, and legacy font variable as migration notes.
- [ ] Keep `XRizer-tz-Custom`, `xrizer/upscaler.json`, `xrizer_scale`, and `XRIZER_UPSCALER_CONFIG` references unchanged.

### Task 5: Verification, commits, and draft PR

- [ ] Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `cargo build --release`.
- [ ] Run `scripts/build-appimage.sh` if `patchelf`, `mksquashfs`, `curl`, and `unzip` are available; otherwise report why it was skipped.
- [ ] Grep for remaining old-name references and classify each as intentional migration/historical code or a missed rename.
- [ ] Commit logical changes, push `chore/rename-tz-controller`, and open a draft PR against `main` in `TommyZ-7/XRizer-tz-Config`.
