# tz-controller AppImage 設計書

> **Historical note (rename C1, 2026-11-06):** この文書は旧名 `XRizer-tz Config` / `xrizer-tz-config` 時代の設計・実装記録です。現在の名前は `tz-controller` です。本文は現在の名前・パスに更新しています。

- 日付: 2026-11-05
- 状態: 実装済み（2026-11-05、v0.1.2 でフォント構成を修正）
- 対象: `TommyZ-7/tz-controller`
- ライセンス: GPL-3.0-or-later（同梱フォントは SIL OFL 1.1）

## 1. 目的

XRizer-tz-Custom の設定 GUI を、ビルド済み Linux 環境へ配布しやすい 1 ファイルの AppImage として生成できるようにする。ローカルの CachyOS でも GitHub Actions の Ubuntu 22.04 でも同じスクリプトでビルドし、AppImage が実際に起動することを検証する。

## 2. 決定事項

1. **配布互換性**: GitHub Actions の `ubuntu-22.04`（glibc 2.35）でビルドする。同じ `scripts/build-appimage.sh` をローカルでも実行可能にする。
2. **ライブラリ同梱**: `linuxdeploy` で非コアライブラリを AppDir に同梱する。glibc / libGL / libEGL / libdrm などのコア graphics ライブラリはホストに任せる。
3. **アイコン**: 仮アイコンを生成して同梱する。後で差し替え可能にする。
4. **配布**: `v*` タグ push で GitHub Release に AppImage を添付する。`workflow_dispatch` でも artifact を保存する。
5. **日本語フォント**: Noto Sans CJK JP を AppImage に同梱し、egui の `FontDefinitions` に直接読み込む。ホストの fontconfig / システムフォントには依存しない。SIL OFL 1.1 のライセンスを同梱する。

## 3. ゴール / 非ゴール

### ゴール

- `scripts/build-appimage.sh` で `dist/tz-controller-<version>-x86_64.AppImage` を生成できる。
- 生成物を `--appimage-extract-and-run` で起動でき、通常の FUSE2 環境ではダブルクリック相当で起動できる。
- Ubuntu 22.04 CI でビルド・起動スモークテスト・artifact 保存ができる。
- タグ push で Release に AppImage が添付される。
- 日本語 UI が AppImage 内のフォントで表示される。
- 既存の `cargo test` / clippy / fmt / release build を壊さない。

### 非ゴール

- aarch64 / armhf への対応。
- Windows / macOS。
- AppImage の自動更新（zsync / update information）。
- GPG 署名。
- Flatpak / Snap / deb / rpm。
- アプリ本体の機能変更（AppImage 化に必要な最小限のビルド設定を除く）。

## 4. アプローチ

### 採用: linuxdeploy + appimagetool

- `cargo build --release --locked` でバイナリを生成。
- AppDir にバイナリ、`.desktop`、512x512 アイコン、Noto CJK フォント、AppRun を配置。
- `linuxdeploy --appdir ... --executable ... --desktop-file ... --icon-file ... --custom-apprun ...` で依存ライブラリを同梱。
- `appimagetool` で AppImage を生成。
- CI とローカルで同じスクリプトを使う。

### 却下した案

- **最小 AppDir（バイナリのみ）**: 小さいが、libxkbcommon / X11 / Wayland などの不足で環境依存になる。ユーザーの「1 ファイルで動く」期待に合わない。
- **Docker / appimage-builder**: 再現性は高いが、このリポジトリの CI とローカルに追加のコンテナ依存を持ち込む。linuxdeploy で十分。
- **フォントを systems font に任せる**: 日本語 UI が tofu になる環境があるため、方針 5-B に反する。

## 5. 生成物とファイル構成

```
assets/appimage/
├── AppRun
├── io.github.tommyz7.TzController.desktop
├── tz-controller.svg          # 仮アイコンの原本
└── tz-controller.png          # 512x512、AppDir 用
assets/fonts/
└── OFL.txt                       # Noto CJK の SIL OFL 1.1
scripts/
└── build-appimage.sh
.github/workflows/
└── appimage.yml
```

- AppImage 名: `tz-controller-<version>-x86_64.AppImage`
- desktop ID: `io.github.tommyz7.TzController`
- AppDir 内:
  - `usr/bin/tz-controller`
  - `usr/share/applications/io.github.tommyz7.TzController.desktop`
  - `usr/share/icons/hicolor/512x512/apps/io.github.tommyz7.TzController.png`
  - `usr/share/fonts/noto/NotoSansCJK-Regular.ttc`（または JP OTF）
  - `AppRun`

## 6. AppRun とフォント

`AppRun` は以下を行う。

- `APPDIR` を自身のディレクトリに設定
- `PATH` に `usr/bin`
- `LD_LIBRARY_PATH` に `usr/lib` と `usr/lib/x86_64-linux-gnu`
- AppDir 内の Noto CJK フォントを探し、`TZ_CONTROLLER_FONT` に設定
- `exec usr/bin/tz-controller "$@"`

アプリ側:

- `main.rs` は `TZ_CONTROLLER_FONT` が存在する場合、`NativeOptions::system_font_fallback` を `false` にする。
- `App::new` はそのフォントを `egui::FontDefinitions` に追加し、Proportional / Monospace のフォールバックにする。
- これにより AppImage はホストの fontconfig / システムフォントに依存せず、同梱 Noto CJK だけで日本語を表示する。Ubuntu 22.04 の同梱 libfontconfig と新しいホスト設定の互換性問題も避けられる。

## 7. ビルドスクリプト

`scripts/build-appimage.sh`:

1. リポジトリルートと version を解決（`VERSION` 環境変数で上書き可能）。
2. `cargo build --release --locked`。
3. ツールを `target/appimage-tools/` にキャッシュ:
   - linuxdeploy `1-alpha-20251107-1`
   - appimagetool `1.9.1`
   - それぞれ SHA256 を検証。
   - 実行時は `APPIMAGE_EXTRACT_AND_RUN=1` を設定し、FUSE2 がない環境でもビルドできるようにする。
4. Noto CJK フォントを探す:
   - `/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc`
   - `/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc`
   - `/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc`
   - `fc-match -f '%{file}' 'Noto Sans CJK JP'`
   - 見つからなければ pinned `Sans2.004` の `06_NotoSansCJKjp.zip` をダウンロードし、`NotoSansCJKjp-Regular.otf` を抽出（SHA256 検証）。
5. AppDir を `target/appimage/AppDir` に構築。
6. `linuxdeploy` で依存ライブラリを同梱。
7. `appimagetool` で `dist/tz-controller-<version>-x86_64.AppImage` を生成。
8. 生成物のサイズと `--appimage-extract` の内容を表示。

- 既存の `dist/` は `.gitignore` に追加。
- `target/appimage-tools` と `target/appimage` は `target/` 配下なので既に ignore 済み。

## 8. CI

`.github/workflows/appimage.yml`:

- トリガー: `workflow_dispatch`、`push: tags: ['v*']`
- `runs-on: ubuntu-22.04`
- `permissions: contents: write`
- 依存:
  - ビルド: `libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libwayland-dev libx11-dev libgl1-mesa-dev libegl1-mesa-dev`
  - AppImage: `patchelf`, `squashfs-tools`, `libfuse2`, `fonts-noto-cjk`
  - スモークテスト: `xvfb`, `xdotool`, `imagemagick`, `x11-utils`
- 手順:
  1. checkout / Rust stable
  2. `bash scripts/build-appimage.sh`
  3. Xvfb 上で AppImage を通常起動
  4. `timeout 30 xdotool search --sync --name "tz-controller"` でウィンドウを確認
  5. スクリーンショットを取得
  6. artifact `tz-controller-appimage` をアップロード
  7. タグ push の場合は `gh release create` / `gh release upload --clobber` で Release に添付
- 手動実行時は artifact のみ。タグ時は artifact + Release。

## 9. 検証

### ローカル

1. `bash scripts/build-appimage.sh`
2. `--appimage-extract` で desktop / icon / font / libs を確認。
3. `APPIMAGE_EXTRACT_AND_RUN=1 ./dist/*.AppImage` を `DISPLAY=:0` で起動。
4. `xprop` で `tz-controller` ウィンドウを確認。
5. `import` でスクリーンショットを取得し、日本語 UI と同梱フォントを確認。
6. 可能ならローカルに `libfuse.so.2` を用意し、通常の AppImage 実行も確認。
7. `ldd` で missing がないことを確認。

### CI

- Ubuntu 22.04 でビルド。
- Xvfb + libfuse2 で通常の AppImage 実行。
- xdotool でウィンドウ確認、スクリーンショットを artifact に保存。
- 失敗時は AppImage のログとスクリーンショットを artifact に保存。

## 10. リスクと制限

- **glibc 互換性**: CI は Ubuntu 22.04（glibc 2.35）基準。それより古い distro では動かない可能性がある。
- **FUSE2**: AppImage type 2 は FUSE2 が必要。無い環境では `APPIMAGE_EXTRACT_AND_RUN=1` を使う。README に明記する。
- **グラフィックス**: `linuxdeploy` が libGL / libEGL / libdrm を同梱するとホストと衝突する可能性がある。コア graphics ライブラリは除外し、テストで確認する。
- **フォント**: 同梱 TTC の最初の face が日本語を含むことをスクリーンショットで確認する。別の face が必要になった場合は JP OTF に切り替える。
- **ファイルダイアログ**: `rfd` は xdg-desktop-portal を使う。AppImage 外のシステムサービスに依存する。
- **AppImage の自動更新 / 署名**: 初期スコープ外。
- **CI のフォント取得**: `fonts-noto-cjk` の配置が Ubuntu 22.04 で想定と違う場合は、ビルドスクリプトの検索パスを追加する。

## 11. 完了条件

- `scripts/build-appimage.sh` がローカルで成功し、`dist/` に AppImage ができる。
- AppImage を起動し、ウィンドウが表示され、日本語 UI が確認できる。
- GitHub Actions の AppImage ワークフローが成功し、artifact がダウンロードできる。
- タグ push で Release に AppImage が添付される。
- README にビルド方法、起動方法、FUSE2 の注意、フォント同梱、既知の制限を記載する。
- 既存のテスト・fmt・clippy・release build が通る。
