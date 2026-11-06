# tz-controller 設計書

> **Historical note (rename C1, 2026-11-06):** この文書は旧名 `XRizer-tz Config` / `xrizer-tz-config` 時代の設計・実装記録です。現在の名前は `tz-controller` です。本文は現在の名前・パスに更新しています。

- 日付: 2026-11-05
- 状態: 設計レビュー待ち
- リポジトリ: `TommyZ-7/tz-controller`（public、新規）
- ライセンス: GPL-3.0-or-later

## 1. 目的と背景

XRizer-tz-Custom の実験的空間アップスケーラと学習用キャプチャは、設定ファイル、スケールファイル、モデルファイル、Vulkan レイヤー、Steam 起動オプションという複数の入口を持つ。現状は手作業で JSON やシェル文字列を編集する必要があり、次の失敗が起きやすい。

- 項目の意味や有効範囲が分からず、意図しない値になる
- 編集したファイルが本体の読むパスと違って反映されない
- 壊れた JSON を保存してしまい、本体が設定を読めなくなる
- `ml` に必要なモデルや Vulkan レイヤーの導入状態が分からない
- Steam 起動オプションの環境変数を間違える

本アプリは、これらを 1 つのデスクトップ GUI で安全に扱うことを目的とする。見た目の装飾より、項目の意味が分かること、変更が確実に反映されること、壊れた設定ファイルを出さないことを優先する。

## 2. ゴール / 非ゴール / アンチゴール

### ゴール

1. `~/.config/xrizer/upscaler.json`（実際は本体の優先順位で決まる実効パス）の全項目を編集できる。
2. 未知のキーを保持したまま書き戻す。既存 JSON が壊れている場合は上書きせず、警告と修正手段を出す。
3. `xrizer_scale` を、本体が実際に読むパスへ書く。候補パスと実効パスを画面に表示する。
4. `.xru` モデルを選択し、内容を検証して `~/.config/xrizer/upscaler_model.xru` へ安全に配置する。
5. Vulkan レイヤーの導入状態を表示し、同梱したインストーラで導入・削除できる。
6. `XRIZER_CAPTURE` 系と `XRIZER_UPSCALER_VK_LAYER` を含む Steam 起動オプション文字列を生成し、コピーできる。
7. ファイル書き込みは原子的に行い、失敗時に元ファイルを壊さない。必要に応じて `.bak` を残す。
8. コアロジックを UI から分離し、ユニットテストで安全性を検証する。

### 非ゴール

- XRizer 本体のビルドやゲームの起動。
- VR 内オーバーレイ、ゲーム画面のプレビュー、リアルタイム性能表示。
- Windows / macOS 対応。Linux（X11 / Wayland）のみ。
- `upscaler.json` 以外の XRizer 設定（輝度 `xrizer_brightness` など）の編集。
- 未知キーの削除・並べ替え UI。
- 自動アップデート、テレメトリ、クラウド同期。

### アンチゴール

- 壊れた JSON を「修復」と称して自動上書きすること。
- ユーザーが選んでいないパスへ設定を書き込むこと。
- 起動オプションにシェルインジェクション可能な文字列を生成すること。
- レイヤー導入を root 権限で行うこと。
- 既存の `.xru` やスケールファイルを無確認で破壊すること。

## 3. 制約

- XRizer-tz-Custom 本体の仕様は `~/work-saeki/xrizer` の `main`（設計時点 `e6fbdd4`）に合わせる。
  - 設定: `src/upscaler/mod.rs` の `candidate_paths` / `maybe_reload`
  - スケール: `src/settings.rs` の `scale_file_candidates` / `maybe_reload_scale` / `persist_scale`
  - モデル: `src/upscaler/model.rs` の `default_model_path` / `parse`
  - レイヤー: `scripts/install-vulkan-layer.sh` / `vulkan-layer/manifest.json.in`
  - キャプチャ: `src/capture.rs` の `CaptureSettings::from_env`
- 本体の実装が変わった場合は、パス候補とバリデーションを追随させる必要がある。設計書に本体側の根拠を明記する。
- GUI は Rust + eframe 0.37（egui）。単一クレート構成。
- Linux のみ。Wayland と X11 の両方を有効にする。
- レイヤー導入はユーザーディレクトリのみ（`$XDG_DATA_HOME`）。root は使わない。
- 同梱スクリプト / manifest は XRizer の GPL-3.0-or-later コードに由来するため、アプリ全体を GPL-3.0-or-later とする。

## 4. 採用したアプローチ

### 4.1 構成: 単一クレート + 純粋なコアモジュール

`src/core/` にファイル操作と検証を集約し、`src/ui/` は状態を表示して操作をコア関数へ渡すだけにする。UI テストなしでも、安全要件の大半を `cargo test` で検証できる。

却下した案:

- **ワークスペース分割（core ライブラリ + GUI）**: 分離は強いが、この規模ではビルド・配布・バージョン管理の複雑さが利点を上回る。
- **UI とファイル操作を 1 ファイルに直書き**: 実装は速いが、未知キー保持、壊れた JSON の保護、原子的書き込みの回帰テストが難しくなる。

### 4.2 保存方式: 明示保存（A）

- 変更はフォーム上で保持し、保存ボタンまたは `Ctrl+S` で書き込む。
- 未保存状態を常に表示する。保存に成功したときだけ未保存表示を消す。
- 自動保存は実装しない。スライダー操作中のための中間ファイルを出さない。
- 保存は「設定」と「スケール」で独立して dirty 管理し、全体保存は両方を順に処理する。片方が壊れていても、もう片方は保存できる。

### 4.3 スケールの書き込み先: 実効パスだけ（A）

- 本体の読み込み順を再現し、最初に「存在して数値として読める」ファイルを実効パスとする。
- 実効パスがある場合はそこだけに書く。全候補には書かない。
- 有効なファイルが 1 つもない場合、新規作成先は永続候補（`~/.config/xrizer_scale`、`~/.local/share/xrizer_scale`）を揮発候補（`$XDG_RUNTIME_DIR`、`/tmp`）より優先する。ただし、より上位の候補が存在しない / 不正で、その永続候補を作成すれば実際に読まれる場合に限る。永続候補が書けない場合だけ揮発候補を使い、警告する。
- 実効パスが揮発候補で、下位に有効な永続ファイルがある場合は「ログアウト後は下位ファイルの値に戻る」と警告する。下位にも無い場合は「ログアウト後は既定値 1.0 に戻る」と警告する。
- 既存ファイルがすべて不正で、書き込み可能な未作成候補もない場合は、既存ファイルを上書きせずエラーにする。
- 候補一覧と各候補の状態（有効 / 不正 / 未作成 / 書込不可）を画面に表示する。

### 4.4 ライセンス: GPL-3.0-or-later

同梱する `install-vulkan-layer.sh` と `manifest.json.in` は XRizer（GPL-3.0-or-later）由来。アプリ全体を GPL-3.0-or-later とし、同梱物の出所とハッシュを `assets/README.md` に記録する。

## 5. 全体構成

```
tz-controller/
├── Cargo.toml
├── Cargo.lock
├── LICENSE                     # GPL-3.0
├── README.md                   # 日本語
├── assets/
│   ├── README.md               # 同梱元 commit / sha256 / ライセンス
│   ├── install-vulkan-layer.sh # XRizer main から同梱
│   └── manifest.json.in        # XRizer main から同梱
├── docs/superpowers/specs/2026-11-05-tz-controller-design.md
├── src/
│   ├── main.rs                 # eframe 起動、ウィンドウ設定
│   ├── app.rs                  # App 状態、タブ、ポーリング、保存調停
│   ├── core/
│   │   ├── mod.rs
│   │   ├── env.rs              # Env、XDG / HOME 候補解決
│   │   ├── atomic.rs           # 原子的書き込み、バックアップ
│   │   ├── config.rs           # upscaler.json の読み書き、未知キー保持
│   │   ├── scale.rs            # xrizer_scale の候補・実効パス・保存
│   │   ├── model.rs            # XRU1 ヘッダ検証、モデル配置
│   │   ├── layer.rs            # 同梱インストーラ、導入状態、.so 検証
│   │   ├── launch.rs           # 起動オプション生成、クォート、検証
│   │   ├── settings.rs         # GUI 自身の設定
│   │   └── process.rs          # バックグラウンド実行（レイヤー導入・モデルコピー）
│   └── ui/
│       ├── mod.rs
│       ├── theme.rs            # ダークテーマ、文字サイズ、余白
│       ├── widgets.rs          # ラベル付き行、バッジ、警告バナー、確認ダイアログ
│       ├── upscaler.rs
│       ├── scale.rs
│       ├── model.rs
│       ├── layer.rs
│       ├── launch.rs
│       └── info.rs
└── .github/workflows/ci.yml
```

### 5.1 依存クレート（最小限）

- `eframe = 0.37`（`default-features = false`、`clipboard` / `default_fonts` / `glow` / `system_fonts` / `wayland` / `x11`）
- `serde`（derive）、`serde_json`（`preserve_order`）
- `rfd = 0.17`（既定の xdg-portal + wayland。GTK 開発パッケージ不要）
- `tempfile = 3`（同梱スクリプトの展開、原子的書き込みの一時ファイル）
- 追加の非同期ランタイムは使わない。バックグラウンド処理は `std::thread` + `mpsc`。

### 5.2 状態とデータフロー

```
起動
  └─ Env::from_process()
       ├─ ResolvedPaths（config / scale / model / layer / app settings）
       ├─ ConfigState（Loaded | Missing | Broken）
       ├─ ScaleState（候補一覧 + 実効値）
       ├─ ModelState（既定パスのモデル情報）
       ├─ LayerState（.so、導入状態）
       └─ LaunchState（前回の起動オプション設定）

UI 操作
  └─ フォーム更新（dirty を再計算）
       └─ 保存（Ctrl+S / 保存ボタン）
            ├─ config: 検証 → 生 JSON に既知キーを上書き → 原子的書き込み
            └─ scale: 実効パス再評価 → 値検証 → 原子的書き込み

1 秒ごとのポーリング
  ├─ config の内容ハッシュを比較
  │    ├─ 未編集なら自動再読込
  │    └─ 編集中なら競合バナー（再読込 / このまま保存）
  ├─ scale 候補と実効値を再評価（未編集なら表示を更新）
  └─ レイヤー導入状態を再評価
```

## 6. 画面構成

### 6.1 全体

- 1 ウィンドウ。初期サイズ 1100x760、最小 900x600。
- 左サイドバーで 6 セクションを切り替える。
  1. アップスケーラ
  2. 解像度スケール
  3. モデル
  4. Vulkan レイヤー
  5. 起動オプション
  6. 情報
- 上部バー: アプリ名、設定 / スケール / レイヤーの状態バッジ、`再読み込み`、`未保存` 表示、`保存` ボタン。
- 下部ステータスバー: 設定ファイルの実効パス、スケールの実効パス、最後の保存結果。
- サイドバーの各項目に状態バッジ（例: 設定 `正常`、レイヤー `導入済`）を出す。
- 破損・競合・保存失敗は、該当タブ内の持続バナーと上部バッジの両方で示す。トーストだけに頼らない。

```
┌──────────────────────────────────────────────────────────────────────┐
│ tz-controller   [設定: 正常] [スケール: 70%] [レイヤー: 未導入]   │
│                                      [再読み込み] [● 未保存] [保存] │
├───────────────┬──────────────────────────────────────────────────────┤
│ アップスケーラ │  アップスケーラ                                       │
│ 解像度スケール │  [x] アップスケーラを有効にする                       │
│ モデル         │  プリセット  ( ) fast  (•) balanced  ( ) fsr  ( ) ml  │
│ Vulkan レイヤー│  ...                                                  │
│ 起動オプション │                                                      │
│ 情報           │                                                      │
├───────────────┴──────────────────────────────────────────────────────┤
│ 設定: ~/.config/xrizer/upscaler.json   スケール: /run/user/1000/...   │
└──────────────────────────────────────────────────────────────────────┘
```

### 6.2 共通操作

- `Ctrl+S`: 全体保存。設定が壊れている場合はスケールだけ保存し、その旨を表示する。
- `F5`: 再読み込み。未保存変更がある場合は確認ダイアログを出す。
- 各数値はスライダーだけでなく `DragValue` で直接入力できる。単位と範囲をラベルに明記する。
- 各項目に `?` ツールチップを付ける。範囲・既定値・本体での意味を日本語で説明する。
- 破壊的操作（レイヤー削除、モデル上書き、未保存変更の破棄）は確認ダイアログを出す。
- 保存成功時は「保存しました」、失敗時は理由を表示する。成功トーストは 3 秒で消す。エラーは消さない。

### 6.3 アップスケーラタブ

- 先頭に `enabled` の大きなスイッチ。
- プリセットは 4 つのラジオカード。各カードに短い説明と、フォールバック条件を表示する。
  - `fast`: バイリニア + 簡易シャープ。最も軽い。
  - `balanced`: SGSR 1 + 適応シャープ。既定。反転 bounds では `fast` にフォールバック。
  - `fsr`: FSR 1 EASU + RCAS。AA / トーンマップ済み入力向け。
  - `ml`: FSR 1 + 学習モデル。モデル・DP4A・Vulkan レイヤーが必要。使えない場合は `fsr` にフォールバック。
- 強調の重み場:
  - `center` X / Y（0.0–1.0）: 2 つのスライダー + 「中央に戻す」。
  - `radius`（0.05–2.0）、`feather`（0.0–1.0）、`strength`（0.0–3.0）。
  - 右側に 160x160 の重み場プレビューを表示する。正方形・アスペクト補正なしの簡易表示であることを明記。中心、radius の円、radius+feather の外側を描く。
  - 「radius まで最大、radius+feather で 0。radius=2.0 で実質全面」の説明を常設。
- 出力とフラグ:
  - `output_scale`（0.5–2.0）。「ランタイム推奨サイズへの倍率」と表示。
  - `preserve_alpha`、`compare`、`show_region` のスイッチ。`compare` は 2 秒ごとの A/B、`show_region` は radius の等高線表示であることを説明。
- 自動スケール:
  - `auto_scale` スイッチ、`min_scale` / `max_scale`（0.1–4.0）。
  - `min > max` の場合は本体が入れ替えて解釈することを表示。UI では保存時に正規化する。
  - `auto_scale` が有効なとき、本体が `xrizer_scale` を 0.05 刻みで書き換える可能性がある旨を表示。
- `model_path`:
  - テキスト入力 + `クリア` ボタン。`null` の場合は既定モデルを使うことを説明。
  - 空文字は `null` として扱う。先頭の `~` は `$HOME` に展開し、絶対パスを必須にする。相対パスはエラー表示し、設定の保存をブロックする。
  - 値が既定パス以外を指す場合、モデルタブの「既定パスへ配置」が効かない旨の警告を出す。
- 下部に「このタブを既定値に戻す」ボタン（フォームのみ変更、保存は別）。
- 「未知のキー」コラプシブル: 保持される未知キーの件数と JSON を読み取り専用で表示する。

### 6.4 解像度スケールタブ

- 大きなスライダー（0.1–4.0、0.05 刻み）と数値入力、百分率表示。
- クイックボタン: 50% / 70% / 85% / 100% / 150%。
- 「推奨開始点: 70%（ゲームが 70% で描画し、XRizer が推奨サイズへ拡大）」の説明。
- 実効パスの大きな表示と、その理由（既存の有効ファイル / 新規作成）。
- 候補パス一覧（優先順）:
  - パス
  - 状態: `有効` / `不正（スキップ）` / `未作成` / `読めない` / `書込不可`
  - 現在値（読めた場合）
- 上位候補が不正で下位が有効な場合は「上位の不正ファイルは無視され、このファイルが読まれます」と警告。
- 実効パスが `$XDG_RUNTIME_DIR` または `/tmp` の場合は「ログアウト / 再起動で消える可能性があります」と注意。
- 保存はこのタブの `スケールを保存` でも、上部の `保存` でもよい。値は 3 桁固定（例: `0.700`）、改行なしで書く。

### 6.5 モデルタブ

- 既定パス `~/.config/xrizer/upscaler_model.xru` の現在のモデル情報:
  - 有無、サイズ、更新日時
  - 種別（MLP / Residual CNN / Offset）、feature count、hidden / channels、レイヤー数、レイヤー形状
  - 壊れている場合はエラー内容
- ソース選択:
  - パスのテキスト入力 + `参照...`（rfd）。
  - 選択時に XRU1 ヘッダを検証し、種別・形状・サイズを表示。
  - 不正なら理由を表示し、`既定パスへ配置` を無効化。
- `既定パスへ配置`:
  - 既存ファイルがある場合は「`.bak` に退避して置き換える」確認。
  - バックグラウンドでコピー。コピー中はスピナー。
  - 完了後に現在のモデル情報を再読込。
- `model_path` が既定パス以外を指している場合:
  - 警告と `model_path を null に戻す` ボタン（設定フォームを変更。保存は別）。
- `ml` プリセットが選ばれていない場合:
  - 「モデルを配置しても、プリセットが ml でない限り使われません」と表示。
- モデルの内容は実行時に本体が検証する。GUI の検証は早期警告であり、本体のロードを保証しない旨を表示。

### 6.6 Vulkan レイヤータブ

- 説明: `ml` の DP4A 経路にのみ必要。`fast` / `balanced` / `fsr` には不要。失敗時は本体が FSR にフォールバックする。
- `.so` の選択:
  - パスのテキスト入力 + `参照...`。
  - 自動検出候補: `~/work-saeki/xrizer/target/release/libxrizer_upscaler_layer.so`、`~/work-saeki/xrizer/target/debug/libxrizer_upscaler_layer.so`（存在するもの）。
  - 検証: 絶対パス、通常ファイル、ELF マジック、サイズ、更新日時。`|` と改行を含むパスは同梱スクリプトの `sed` が扱えないため拒否する。
- 導入状態:
  - `導入済み`: manifest とライブラリの両方が存在し、manifest の layer 名が `VK_LAYER_XRIZER_upscaler` で、`library_path` のファイルが存在する。
  - `不完全`: manifest のみ、またはライブラリのみ。
  - `未導入`: どちらもない。
  - 表示: manifest パス、インストール済みライブラリパス、サイズ、更新日時。
  - 選択中の `.so` と導入済み `.so` のサイズ / 更新日時が違う場合は「再インストールしてください」と表示。
- 操作:
  - `インストール / 更新`: 同梱スクリプトを一時ディレクトリへ展開し、`XRIZER_LAYER_LIB=<選択した .so>` を設定して `bash install-vulkan-layer.sh --user` を実行。
  - `削除`: 同じスクリプトで `--remove`。確認ダイアログ。
  - 実行中はボタンを無効化し、スピナーと「実行中...」を表示。
  - 標準出力 / 標準エラーを折りたたみログに表示。
- 注意:
  - 導入状態はファイルの存在で判定する。実際に Vulkan ローダーが読んだかは `VK_LOADER_DEBUG=layer` で確認する必要がある。
  - レイヤーはゲーム起動時に読まれるため、導入・削除後はゲームを再起動する。
  - 起動オプションで `XRIZER_UPSCALER_VK_LAYER=1` を有効にする必要がある。
- 同梱元:
  - XRizer `main` `e6fbdd4` の `scripts/install-vulkan-layer.sh` と `vulkan-layer/manifest.json.in`。
  - `assets/README.md` に sha256 とライセンスを記録。
  - アプリの情報タブにも同梱元 commit を表示。

### 6.7 起動オプションタブ

- 「これらの環境変数はゲーム起動時に読まれます。変更後はゲームを再起動してください」と明記。
- Vulkan レイヤー:
  - 3 択: `指定しない` / `有効 (XRIZER_UPSCALER_VK_LAYER=1)` / `強制無効 (XRIZER_UPSCALER_VK_LAYER_DISABLE=1)`。
  - `ml` プリセットなのに有効化していない場合は警告。レイヤー未導入で有効化した場合も警告（保存は可能）。
- キャプチャ:
  - `キャプチャを有効にする` スイッチ。
  - 保存先ディレクトリ（テキスト + フォルダ選択）。絶対パス必須。`~` は GUI 側で展開する。存在しない場合はゲームが作成する旨を表示。
  - モード: `interval` / `shutter`。
    - interval: 間隔 ms（100–60000、既定 500）
    - shutter: 長押し ms（100–500、既定 250）、ロックアウト ms（200–5000、既定 1000）
  - 最大ペア数（1–100000、既定 300）。
  - シーン名（任意。空なら `XRIZER_CAPTURE_SCENE` を出さない）。
  - shutter の説明: 両手のグリップ + トリガーを同時に保持。1 回で 1 ペア、ロックアウト付き。
- 上級者向け（既定 OFF。ただしレイヤーパス固定は「有効 + 導入済み」で ON）:
  - `設定ファイルのパスを固定する`: `XRIZER_UPSCALER_CONFIG=<GUI が編集している実効パス>` を追加。
  - `スケールファイルのパスを固定する`: `XRIZER_SCALE_FILE=<GUI が書く実効パス>` を追加。
  - `レイヤーの探索パスを固定する`: `VK_ADD_IMPLICIT_LAYER_PATH=<導入済み manifest のディレクトリ>` を追加。
  - 目的: Steam Linux Runtime などでゲーム側の `HOME` / `XDG_*` が GUI と違う場合に、同じファイル / manifest を読ませる。
  - 注意: パスを固定すると、ゲーム側の環境変数より優先される。上級者向けであることと、Vulkan loader 1.3.296+ が必要であることを明記。
- 生成結果:
  - 等幅・選択可能な 1 行テキスト。
  - `%command%` を必ず末尾に置く。
  - 環境変数は安定した順序で出力する。
  - 値を POSIX シェル向けにクォートする。安全な文字列はそのまま、それ以外はシングルクォートし、値内の `'` は `'\''` に変換する。NUL / 改行は拒否する。
  - `コピー` ボタンと `環境変数だけコピー` ボタン。
  - バリデーションエラーがある場合はコピーを無効化し、理由を表示する。
- 生成例:

```
XRIZER_CAPTURE=1 XRIZER_CAPTURE_DIR='/home/user/xrizer-captures/VRChat/scene-a' XRIZER_CAPTURE_MODE=interval XRIZER_CAPTURE_INTERVAL_MS=500 XRIZER_CAPTURE_MAX_PAIRS=300 XRIZER_UPSCALER_VK_LAYER=1 %command%
```

### 6.8 情報タブ

- アプリのバージョン、ビルドプロファイル、ライセンス。
- 解決したパス:
  - 設定ファイル（実効 / 候補一覧）
  - スケール（実効 / 候補一覧）
  - モデル既定パス
  - レイヤー manifest / ライブラリ
  - GUI 設定ファイル
- 環境値: `HOME`、`XDG_CONFIG_HOME`、`XDG_RUNTIME_DIR`、`XDG_DATA_HOME`、`XRIZER_UPSCALER_CONFIG`、`XRIZER_SCALE_FILE`。
  - 「ゲーム側の環境と異なる場合がある」と明記。
- 同梱インストーラの出所 commit と sha256。
- `設定ファイルをエディタで開く` / `設定ディレクトリを開く`（`xdg-open`、ベストエフォート）。

## 7. ファイル安全設計

### 7.1 原子的書き込み

`core::atomic::write_atomic(path, bytes, backup: BackupPolicy)` を共通利用する。

1. 親ディレクトリを作成する（存在しない場合）。
2. 対象がシンボリックリンクなら、リンク先の実体パスを `canonicalize` して書き込み先にする。リンク自体は残す。
3. 対象が通常ファイル以外（ディレクトリ、デバイス等）ならエラー。
4. 同じディレクトリに一時ファイル `<name>.tmp.<pid>.<counter>` を作る。
5. バイト列を書き、`sync_all()` する。
6. 既存ファイルのパーミッションを一時ファイルへコピーする。新規なら既定（umask 準拠）。
7. 書き込み前に内容を検証する（設定は JSON 再パース、スケールは f32 再パース、モデルは XRU ヘッダ再検証）。
8. `BackupPolicy::BeforeReplace` の場合、既存ファイルを `<name>.bak` へコピーする。バックアップに失敗したら保存を中止する。
9. 一時ファイルを対象へ `rename` する。
10. 親ディレクトリを `File::open` + `sync_all()` する（ベストエフォート。失敗は警告）。
11. 失敗時は一時ファイルを削除し、元ファイルを変更しない。

### 7.2 壊れた設定ファイル

- 読み込み時に次のいずれかなら `Broken` 状態にする。
  - UTF-8 でない
  - JSON として不正
  - トップレベルがオブジェクトでない
  - 既知キーの型が違う（`serde_json::from_value` が失敗）
- `Broken` のとき:
  - 上部に赤い持続バナー。パス、エラー内容、生ファイルのプレビュー（先頭 64 KiB）を表示。
  - 設定タブの編集控件を無効化。
  - 設定の保存を無効化。スケール・モデル・レイヤー・起動オプションは利用可能。
  - `再読み込み` と `エディタで開く` を出す。
  - 絶対に自動で上書き・修復しない。
- 本体は「最初に存在する設定ファイル」が壊れている場合、下位のファイルへフォールバックせず、最後に正常だった設定を使い続ける。GUI も同じファイルを対象にし、下位ファイルへ勝手に切り替えない。

### 7.3 未知キーの保持

- 生 JSON を `serde_json::Map<String, Value>` として保持する。
- `serde_json` の `preserve_order` を有効にし、既存キーの順序を保つ。
- 保存時は生マップを複製し、既知の 14 キーだけを現在のフォーム値で上書きする。未知キーは値も順序もそのまま残す。
- 既知キーが元ファイルに無い場合は、保存時に全既知キーを追加する。
- 未知キーの件数と内容を読み取り専用で表示する。

### 7.4 外部変更と競合

- 1 秒ごとに設定ファイルを読み、内容ハッシュ（`DefaultHasher`）を比較する。
- 未編集（dirty でない）:
  - 変更を自動で再読込し、トーストで知らせる。
  - 壊れた内容になった場合は `Broken` に遷移する。
- 編集中（dirty）:
  - 競合バナーを出し、自動再読込しない。
  - 選択肢: `再読み込み（未保存の変更を破棄）` / `このまま保存（外部変更を上書き）`。
  - 上書き保存を選んだ場合も、保存前に原子的書き込みとバックアップを行う。
- 保存直後は自分自身の変更を外部変更と誤認しないよう、ハッシュを更新する。
- ファイルが外部で削除された場合:
  - 未編集なら「設定ファイルが削除されました。保存すると再作成します」と表示。
  - 編集中なら競合バナー。
- スケールファイルも同じ内容ハッシュ比較を行う。
  - dirty でない: 外部変更（`auto_scale` による本体の書き換えを含む）を自動で再読込する。
  - dirty: 競合バナーを出し、`再読み込み（未保存の変更を破棄）` / `このまま保存（外部変更を上書き）` を選ばせる。
  - 保存直前にもう一度内容ハッシュを確認し、読み込み時から変わっていれば、dirty でなくても上書き確認を出す。
- `auto_scale` が有効なときは、本体が `xrizer_scale` を 0.05 刻みで書き換える。スケールタブに常設の警告を出し、手動保存時は「自動調整の値より優先されます」と確認する。
- 設定・スケールとも、保存処理の直前にファイル内容を再確認してから置換する（ポーリングの 1 秒間の空白を埋める）。

### 7.5 バックアップ

- 設定: `<name>.bak`（1 世代）。
- スケール: `<name>.bak`（1 世代）。
- モデル: `upscaler_model.xru.bak`（1 世代）。
- バックアップは保存直前の内容。バックアップ失敗時は保存を中止する。
- `.bak` のパスを保存結果に表示する。

## 8. 設定ファイル（upscaler.json）

### 8.1 パス解決

本体 `src/upscaler/mod.rs` の `candidate_paths()` と同じ順序。

1. `$XRIZER_UPSCALER_CONFIG`
2. `$XDG_CONFIG_HOME/xrizer/upscaler.json`
3. `$HOME/.config/xrizer/upscaler.json`
4. `$XDG_RUNTIME_DIR/xrizer_upscaler.json`
5. `/tmp/xrizer_upscaler.json`

- 重複を除く。
- 実効ファイル = 最初に `exists()` が真になるパス。内容のパース成否では下位へ落ちない。
- 存在するファイルが 1 つもない場合、書き込み可能な最初の候補を新規作成先にする。`XDG_CONFIG_HOME` / `~/.config` がランタイムより優先されるため、通常は `~/.config/xrizer/upscaler.json` になる。
- 空の環境変数は未設定として扱う。
- パスは起動時に解決し、情報タブに表示する。

### 8.2 読み込みと正規化

- 生バイト列を UTF-8 文字列として読む。失敗は `Broken`。
- `serde_json::Value` へパース。失敗は `Broken`。
- トップレベルがオブジェクトでなければ `Broken`。
- 既知キーを `UpscalerConfig` へデシリアライズ。失敗は `Broken`。
- 欠けているキーは既定値。
- 範囲外の値は実行時の sanitize と同じ範囲へ丸め、`notices` に記録する。
  - `center` 各 0.0–1.0
  - `radius` 0.05–2.0
  - `feather` 0.0–1.0
  - `strength` 0.0–3.0
  - `output_scale` 0.5–2.0
  - `min_scale` / `max_scale` 0.1–4.0
  - `min_scale > max_scale` は入れ替え
  - 非有限値は JSON では表現できないため、パース失敗として扱う
- 丸めが発生した場合は「範囲外の値を実行時の値に丸めて表示しています。保存すると丸めた値が書き込まれます」と表示する。

### 8.3 項目と UI

| キー | 型 | 既定 | UI |
|---|---|---|---|
| `enabled` | bool | false | 大きなスイッチ |
| `preset` | string | balanced | ラジオカード |
| `center` | [f32;2] | [0.5,0.5] | X/Y スライダー + 中央に戻す |
| `radius` | f32 | 0.35 | スライダー 0.05–2.0 |
| `feather` | f32 | 0.10 | スライダー 0.0–1.0 |
| `strength` | f32 | 1.0 | スライダー 0.0–3.0 |
| `output_scale` | f32 | 1.0 | スライダー 0.5–2.0 |
| `preserve_alpha` | bool | true | スイッチ |
| `compare` | bool | false | スイッチ |
| `show_region` | bool | false | スイッチ |
| `model_path` | string/null | null | テキスト + クリア |
| `auto_scale` | bool | false | スイッチ |
| `min_scale` | f32 | 0.5 | スライダー 0.1–4.0 |
| `max_scale` | f32 | 1.5 | スライダー 0.1–4.0 |

### 8.4 保存

- 生マップ（未知キー含む）を複製。
- 全既知キーを現在値で挿入。
- `serde_json::to_string_pretty`（2 スペース）+ 末尾改行。
- 原子的書き込み（バックアップあり）。
- 一時ファイルを再パースして `UpscalerConfig` に戻せることを確認してから rename。
- 保存成功後、生マップと保存済みスナップショットを更新し、dirty を解除。
- 保存失敗時は dirty を維持し、エラーを表示。

## 9. 解像度スケール（xrizer_scale）

### 9.1 パス候補

本体 `src/settings.rs` の `scale_file_candidates()` と同じ順序。

1. `$XRIZER_SCALE_FILE`
2. `$XDG_RUNTIME_DIR/xrizer_scale`
3. `$HOME/.config/xrizer_scale`
4. `$HOME/.local/share/xrizer_scale`
5. `/tmp/xrizer_scale`

重複を除く。空の環境変数は未設定として扱う。

### 9.2 実効パスと値

各候補を次のように分類する。

- `Valid(value)`: 読み込めて `f32` としてパースでき、本体と同じ正規化を適用できる。
- `Invalid(reason)`: 存在するが読めない / パースできない。
- `Missing`: 存在しない。
- `Unwritable`: 親ディレクトリを作成できない、または書き込み権限がない。

本体の読み込みを再現する。

1. 優先順に `Valid` を探す。最初の `Valid` が実効パス。
2. 値の正規化: `raw > 4.0` なら `raw / 100.0`、その後 `0.1..=4.0` に clamp。例: `70` → `0.70`、`5` → `0.05` → `0.1`。
3. `Valid` が無い場合、永続候補（`~/.config/xrizer_scale`、`~/.local/share/xrizer_scale`）のうち、上位候補がすべて `Missing` / `Invalid` で、作成すれば実際に読まれる最初の `Missing` を新規作成先にする。永続候補が書けない場合だけ、揮発候補（`$XDG_RUNTIME_DIR`、`/tmp`）の最初の `Missing` を使う。
4. `Valid` も `Missing` もなく、既存がすべて `Invalid` の場合は保存先なし。既存ファイルを上書きしない。
5. 上位に `Invalid` があり下位に `Valid` がある場合は、下位が実効。上位は本体に無視される旨を表示。
6. 実効パスが揮発候補の場合、下位の永続 `Valid` を探して「ログアウト後の値」を表示する。無ければ既定値 1.0 に戻る旨を表示する。

### 9.3 表示

- 現在値（百分率 + 小数）、実効パス、理由。
- 候補一覧の表。
- 実効パスが `$XDG_RUNTIME_DIR` / `/tmp` の場合は揮発性の注意。下位に永続 `Valid` があればそのパスと値を「ログアウト後はこの値」として表示する。
- `auto_scale` が有効な場合、本体がこのファイルを書き換える可能性がある旨を常設で表示する。

### 9.4 保存

- 保存直前に候補を再評価する。実効パスが変わっていたら新しい実効パスへ書く。
- 保存直前の内容ハッシュが読み込み時と違う場合、上書き確認を出す。`auto_scale` が有効なら「自動調整の値より優先されます」と明示する。
- 値は `0.1..=4.0` に clamp。表示は 3 桁固定、改行なし（例: `0.700`）。
- 原子的書き込み（バックアップあり）。
- 一時ファイルの内容を `f32` として再パースし、範囲内であることを確認してから rename。
- 保存後、候補一覧と実効値を再評価。

## 10. モデル（.xru）

### 10.1 既定パス

本体 `model::default_model_path()` と同じ。

- `$HOME/.config/xrizer/upscaler_model.xru`
- `$HOME` が無い場合は `/tmp/xrizer_upscaler_model.xru`

`XDG_CONFIG_HOME` は使わない（本体の実装に合わせる）。

### 10.2 XRU1 検証

ファイル全体をメモリに載せず、ヘッダ 64 バイト + レイヤーテーブル（20 バイト × レイヤー数）を読む。ファイルサイズは `metadata.len()` で検証する。

検証項目（本体 `Model::parse` に合わせる）:

- 長さ >= 64、マジック `XRU1`、バージョン 1
- kind 0/1/2（MLP / Residual CNN / Offset）
- feature_count、hidden、channels、layer_count
- `input_scale` / `activation_scale` / `residual_clamp` / `scale` が有限かつ非負、`residual_clamp <= 1.0`
- MLP: feature_count が期待値、hidden が 8/16、2 レイヤー、最終出力 1
- CNN / Offset: 3 レイヤー、channels が期待値
- 各レイヤーの input/output が 0 でない、scale が有限かつ正
- レイヤー形状が連結
- 期待ファイルサイズ = 64 + 20×layer_count + Σ(input×output + output×4)
- 実ファイルサイズと一致（切り詰め・末尾データを拒否）

表示: 種別、feature_count、hidden / channels、レイヤー数、各レイヤー形状、サイズ、更新日時。

### 10.3 配置

- ソースはテキスト入力 + rfd。`.xru` 以外も選択可能だが、検証で拒否。
- ソースと既定パスが同一実体なら配置を無効化。
- 既存の既定パスがある場合は確認ダイアログ。
- バックグラウンドで:
  1. 既定パスの親ディレクトリを作成。
  2. 同ディレクトリに一時ファイルを作る。
  3. ソースをコピーし、`sync_all()`。
  4. 一時ファイルを XRU1 として再検証。
  5. 既存ファイルを `.bak` へコピー。
  6. rename。
  7. ディレクトリを fsync（ベストエフォート）。
- 失敗時は一時ファイルを削除し、既存ファイルを変更しない。

### 10.4 `model_path` との関係

- `model_path` が `null` なら既定パスが使われる。
- `model_path` が文字列の場合は、先頭の `~` を `$HOME` に展開し、絶対パスであることを必須にする。空文字は `null` として扱う。
- 相対パスは本体のゲーム CWD 基準になり、GUI からは正しく解決できないため保存をブロックし、絶対パスを促す。
- モデルタブの「現在使われるモデル」は、`model_path` があればそのパス、なければ既定パスを表示・検証する。
  - `model_path` のファイルが無い / 不正なら警告し、`model_path を null に戻す` ボタンを出す。
  - `model_path` が既定パス以外なら「既定パスへの配置は使われません」と表示する。
- `model_path` が既定パスの場合は、通常の「既定パスへ配置」がそのまま効く。
- 設定が `Broken` の場合は `model_path` を変更するボタンを無効化。
- プリセットが `ml` でない場合、「配置しても使われない」旨を表示。

## 11. Vulkan レイヤー

### 11.1 同梱物

- `assets/install-vulkan-layer.sh`: XRizer `main` `e6fbdd4` の `scripts/install-vulkan-layer.sh` をそのまま同梱。
- `assets/manifest.json.in`: 同 commit の `vulkan-layer/manifest.json.in` をそのまま同梱。
- `assets/README.md`: 出所 URL、commit、sha256、GPL-3.0-or-later である旨。
- 実行時に `include_str!` で埋め込み、一時ディレクトリに次の構造で展開する。

```
<temp>/
├── scripts/install-vulkan-layer.sh  (0755)
└── vulkan-layer/manifest.json.in    (0644)
```

スクリプトは `ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"` を使うため、この構造で `ROOT=<temp>` になる。

### 11.2 .so の選択と検証

- アプリ設定に前回の `.so` パスを保存。
- 初回起動時は次の候補を上から探し、最初に存在するものを初期値にする。
  - `$HOME/work-saeki/xrizer/target/release/libxrizer_upscaler_layer.so`
  - `$HOME/work-saeki/xrizer/target/debug/libxrizer_upscaler_layer.so`
- 検証:
  - 絶対パス
  - 通常ファイル
  - ELF マジック `\x7fELF`
  - NUL / 改行を含まない
  - サイズ > 0
- `.so` パスはスクリプトの `sed` には渡らず、`install` と `XRIZER_LAYER_LIB` に引用符付きで渡るため、`|` などの制限は不要。
- 代わりに `$XDG_DATA_HOME`（未設定なら `$HOME/.local/share`）を検証する。`sed` の置換先になるため、`|` / `&` / `\` / 改行を含む場合はインストールを拒否する。
- 検証エラー時はインストールを無効化。

### 11.3 インストール / 削除

- インストール:
  - 一時ディレクトリへ同梱物を展開。
  - `Command::new("bash").arg(script).arg("--user")`
  - `env("XRIZER_LAYER_LIB", so_path)`
  - 出力をキャプチャ。
  - 生成された manifest をパースし、`layer.name` と `library_path` が期待値（インストール先の `.so`）と一致することを確認する。
  - 不一致 / パース失敗なら、同じスクリプトの `--remove` でロールバックし、エラーを表示する。
  - 成功後に導入状態を再評価。
- 削除:
  - `bash <script> --remove`
  - 確認ダイアログ。
- 実行はバックグラウンドスレッド。UI は `mpsc::Receiver` を毎フレーム poll する。
- 同時実行は 1 つまで。
- 失敗時は終了コード、stdout、stderr を表示。
- 一時ディレクトリは処理後に削除する（`tempfile::TempDir`）。

### 11.4 導入状態

- `$XDG_DATA_HOME`（未設定なら `$HOME/.local/share`）を使う。
- manifest: `$XDG_DATA_HOME/vulkan/implicit_layer.d/xrizer_upscaler_layer.json`
- lib: `$XDG_DATA_HOME/xrizer/vulkan-layer/libxrizer_upscaler_layer.so`
- manifest をパースし、`layer.name == "VK_LAYER_XRIZER_upscaler"` を確認。
- `library_path` が相対なら manifest のディレクトリ基準で解決。絶対ならそのまま。
- ファイルの存在、サイズ、更新日時を表示。
- 状態:
  - `Installed`: manifest と lib が両方有効。
  - `Partial`: 片方のみ、または manifest の参照先が無い。
  - `NotInstalled`: どちらも無い。
  - `Error`: manifest が読めない / JSON が不正。
- 選択中の `.so` と導入済み `.so` の同一性は、サイズ + 内容ハッシュ（FNV-1a 64-bit）で判定する。`install` は mtime を保持しないため、mtime は使わない。
  - 不一致なら「選択中の `.so` と導入済みが異なります。再インストールしてください」と表示する。
  - ハッシュ計算は `.so` 選択時と導入状態の再評価時だけ行い、毎フレームは行わない。

### 11.5 制約

- 導入状態はファイル存在ベース。実際の Vulkan ロードは検証しない。
- レイヤーはゲーム起動時に読まれる。導入・削除後はゲーム再起動が必要。
- `XRIZER_UPSCALER_VK_LAYER=1` が起動オプションに無いと有効にならない。
- スクリプトは `$XDG_DATA_HOME` の外へは書かない。root 不要。

## 12. 起動オプション

### 12.1 環境変数モデル

```rust
enum LayerMode { Unspecified, Enable, Disable }
enum CaptureMode { Interval, Shutter }

struct LaunchForm {
    layer_mode: LayerMode,
    capture_enabled: bool,
    capture_dir: String,
    capture_mode: CaptureMode,
    interval_ms: u64,
    max_pairs: u32,
    scene: String,
    hold_ms: u64,
    lockout_ms: u64,
    pin_config_path: bool,
    pin_scale_path: bool,
    pin_layer_path: bool,
}
```

出力順:

1. `XRIZER_UPSCALER_CONFIG`（pin 有効時）
2. `XRIZER_SCALE_FILE`（pin 有効時）
3. `VK_ADD_IMPLICIT_LAYER_PATH`（layer mode が Enable で pin 有効時。値は導入済み manifest のディレクトリ）
4. `XRIZER_UPSCALER_VK_LAYER=1` または `XRIZER_UPSCALER_VK_LAYER_DISABLE=1`
5. `XRIZER_CAPTURE=1`（キャプチャ有効時）
6. `XRIZER_CAPTURE_DIR`
7. `XRIZER_CAPTURE_MODE`
8. mode に応じた `XRIZER_CAPTURE_INTERVAL_MS` または `XRIZER_CAPTURE_HOLD_MS` / `XRIZER_CAPTURE_LOCKOUT_MS`
9. `XRIZER_CAPTURE_MAX_PAIRS`
10. `XRIZER_CAPTURE_SCENE`（空でない場合）
11. `%command%`

補足:

- `VK_ADD_IMPLICIT_LAYER_PATH` は Vulkan loader 1.3.296+ で利用できる。Steam Linux Runtime などでゲーム側の `XDG_DATA_HOME` が GUI と違う場合に、導入済み manifest を見つけさせるための保険。
- 既定は「レイヤー有効 + 導入済み」のときに ON。チェックボックスで OFF にできる。
- レイヤー未導入で ON にした場合は警告する（コピーは可能）。

### 12.2 バリデーション

- キャプチャ有効時:
  - 保存先が空でない。
  - `~` 展開後、絶対パス。
  - NUL / 改行なし。
  - 数値が本体の clamp 範囲内（UI 側で clamp 済み）。
- レイヤー `Enable` かつレイヤー未導入: 警告（コピーは可能）。
- `ml` プリセットなのに layer mode が `Unspecified`: 警告。
- `VK_ADD_IMPLICIT_LAYER_PATH` を ON にするのに manifest ディレクトリが無い: 警告。
- エラーがある場合のみコピーを無効化。警告はコピーを妨げない。
- 生成文字列は毎フレーム再計算する。

### 12.3 クォート

```rust
fn shell_quote(value: &str) -> Result<String, String>
```

- 空文字は `''`。
- `[A-Za-z0-9_./:@%+=,-]` のみならそのまま。
- それ以外はシングルクォートで囲み、`'` を `'\''` に置換。
- NUL / 改行を含む場合はエラー。
- `%command%` はクォートしない。

テストでは `sh -c` に安全なテスト値を通し、シェルが同じ値を受け取ることを確認する。

### 12.4 コピー

- `ctx.copy_text()` でクリップボードへ。
- `コピー` は Steam 起動オプション全体（`%command%` 含む）。
- `環境変数だけコピー` は `%command%` を除いた文字列。
- コピー後にトースト。

## 13. GUI 自身の設定

- パス: `$XDG_CONFIG_HOME/tz-controller/settings.json`、無ければ `$HOME/.config/tz-controller/settings.json`、無ければ `/tmp/tz-controller/settings.json`。
- 内容:
  - `layer_so_path: Option<String>`
  - `last_model_dir: Option<String>`
  - `launch: LaunchForm`（前回の起動オプション設定）
  - `active_tab: Option<String>`（任意）
- 読み込み失敗時は既定値で起動し、情報タブに警告。壊れた GUI 設定は上書きしない（保存時に `.bak` を作る）。
- 原子的書き込み。設定ファイルほど厳密でなくてよいが、同じヘルパーを使う。

## 14. エラー処理と状態表示

- ファイル I/O エラーはすべて UI に表示する。`unwrap` / `expect` は起動時の定数以外で使わない。
- 保存失敗時は dirty を維持する。
- レイヤー導入・モデルコピーの失敗は、終了コードと出力を表示する。
- パスが読めない / 書けない場合は、その理由（Permission denied など）を日本語ラベル + 元のエラー文字列で表示する。
- 「成功したように見えるが実は失敗」を避けるため、保存後に対象ファイルを再読込して確認する。
  - 設定: 再パース + 既知キーの一致
  - スケール: f32 再パース + 値一致
  - モデル: XRU ヘッダ再検証
- トーストは補助。持続的な状態はバナー / バッジで示す。

## 15. テスト計画

### 15.1 コア（必須）

- `env`
  - 候補順、重複排除、空環境変数、HOME / XDG 欠落。
- `atomic`
  - 新規作成、上書き、バックアップ、パーミッション保持、シンボリックリンク、通常ファイル以外の拒否、失敗時の一時ファイル掃除。
- `config`
  - 既定値、欠損キー、未知キー保持（値・順序）、既知キー上書き、保存 → 再読込の一致。
  - 壊れた JSON、非オブジェクト、型不正、範囲外の丸め、min/max 入れ替え。
  - 実効パス選択（最初の既存ファイル。壊れていても下位へ落ちない）。
  - 保存失敗時に元ファイルが残る。
- `scale`
  - 候補順、最初の Valid、Invalid のスキップ、下位 Valid、新規作成先、全 Invalid 時の拒否。
  - 新規作成時に永続候補を優先し、揮発候補しか書けない場合の警告。
  - 揮発実効パス時の「ログアウト後の値」表示。
  - `70` → `0.70`、`5` → `0.1`、負値 / 0 の clamp。
  - 3 桁固定・改行なしの保存、再読込一致。
  - 外部変更の競合（dirty / not dirty）、保存直前の内容ハッシュ確認。
- `model`
  - MLP / CNN / Offset の正常ヘッダ。
  - 切り詰め、マジック不正、バージョン不正、サイズ不一致、非連結レイヤー、kind ごとの形状不正。
  - コピー、バックアップ、再検証。
  - `model_path` の `~` 展開、空文字 → null、相対パス拒否、カスタムパスの検証。
- `launch`
  - 全パターンの出力順、`%command%` 末尾、クォート（空白、`'`、`"`、`$`、バッククォート）、NUL / 改行拒否、相対パス拒否、pin パス（config / scale / layer）。
  - `VK_ADD_IMPLICIT_LAYER_PATH` の出力と、未導入時の警告。
  - `sh -c` によるクォートのラウンドトリップ。
- `layer`
  - 同梱物の展開構造。
  - 一時 `XDG_DATA_HOME` に対する `--user` / `--remove` の統合テスト（ダミー .so 使用）。
  - manifest 解析、Installed / Partial / NotInstalled / Error の判定。
  - `$XDG_DATA_HOME` に sed 危険文字を含む場合の拒否。
  - 生成 manifest の検証と、失敗時のロールバック。
  - `.so` の内容ハッシュ比較（mtime に依存しない）。
- `settings`
  - 既定値、保存 → 再読込、壊れた設定のフォールバック。

### 15.2 手動確認

- 一時 `HOME` / `XDG_*` で起動し、実ファイルを汚さない。
- 壊れた JSON を置いて起動 → 警告、保存不可、他タブは動作。
- 未知キー入り JSON を編集 → 保存 → 未知キーが残る。
- 外部プロセスで設定を変更 → 未編集なら自動再読込、編集中なら競合バナー。
- スケール候補を複数作り、実効パスが正しく選ばれる。新規作成で永続候補が優先される。
- 外部プロセスでスケールを変更 → 未編集なら自動再読込、編集中なら競合バナー。
- `.xru` を選択 → 検証 → 配置 → 再読込。`model_path` にカスタムパスを設定し、モデルタブがそのファイルを表示する。
- ダミー `.so` でレイヤー導入 → manifest / lib ができ、状態が `導入済み` になる。選択中 `.so` と導入済みのハッシュが一致する。削除で消える。
- 起動オプションで `VK_ADD_IMPLICIT_LAYER_PATH` が manifest ディレクトリを指す。
- 起動オプション文字列をコピーし、`sh -c` で構文エラーがないことを確認（`%command%` は `true` に置換）。
- 1280x800 程度のウィンドウで全文が読める。スクロールが必要な箇所はスクロールできる。

### 15.3 CI

- GitHub Actions `ubuntu-latest`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `cargo build --release`
- 必要パッケージ: `libxkbcommon-dev`、`libwayland-dev`、`libx11-dev`、`libgl1-mesa-dev`、`libegl1-mesa-dev`（ビルド時に不足があれば追加）。
- レイヤー統合テストは `bash` / `sed` / `install` が使える Linux ランナーで実行。

## 16. リポジトリ / 配布

- GitHub `TommyZ-7/tz-controller`（public）。
- ブランチ: `main`。
- 初期コミットに設計書を含める。
- `Cargo.lock` をコミット（バイナリ）。
- `README.md`:
  - 何をするアプリか
  - 前提（Linux、XRizer-tz-Custom、`ml` には Vulkan レイヤー）
  - ビルドと起動
  - 各タブの説明
  - パス優先順位
  - 安全動作（原子的書き込み、`.bak`、壊れた JSON を上書きしない、未知キー保持）
  - レイヤー導入（`.so` の指定、同梱スクリプト、`--user` のみ）
  - 起動オプションの使い方
  - 制限（ゲーム再起動が必要な項目、レイヤーのロードはファイル存在では保証されない）
  - ライセンスと同梱物の出所
- `.gitignore`: `/target`、エディタ一時ファイル。
- `LICENSE`: GPL-3.0 全文。
- `assets/README.md`: 同梱物の出所と sha256。
- リリースバイナリは初期スコープ外。必要なら後続で `cargo build --release` の手順のみ提供。

## 17. 実装順序（概要）

1. リポジトリ骨格、`Cargo.toml`、ライセンス、README の雛形。
2. `core::env` / `core::atomic` + テスト。
3. `core::config` + テスト（未知キー、壊れた JSON、丸め、実効パス）。
4. `core::scale` + テスト（実効パス、書き込み、バックアップ）。
5. `core::model` + テスト（XRU 検証、コピー）。
6. `core::layer` + テスト（同梱物展開、導入 / 削除、状態）。
7. `core::launch` + テスト（生成、クォート、検証）。
8. `core::settings` + テスト。
9. `ui` シェル（テーマ、サイドバー、上部バー、下部ステータス、共通ウィジェット）。
10. 各タブの UI 接続。
11. ポーリング、競合処理、保存調停。
12. README、CI、手動確認。
13. GitHub リポジトリ作成、push。

## 18. builder に委ねる点

- 日本語フォントの読み込み方法（eframe `system_fonts` で不足する場合の Noto Sans JP 同梱可否）。
- テーマの具体的な色・余白・文字サイズ（読みやすさ優先）。
- 重み場プレビューの描画方法（`ColorImage` か円描画か）。
- CI で必要な apt パッケージの最終確定。
- エラーメッセージの日本語表現。
- テスト用の XRU1 バイト列生成ヘルパーの置き場所。

## 19. リスクと制限

- パス優先順位は XRizer `main` `e6fbdd4` 時点の実装に依存する。本体が変わったら追随が必要。
- GUI とゲームの環境変数が違うと、GUI が選んだ実効パスとゲームが読むパスがずれる可能性がある。起動オプションのパス固定（上級者向け）で緩和するが、既定では固定しない。
- `$XDG_RUNTIME_DIR` / `/tmp` のスケールファイルは揮発性。GUI は新規作成時に永続候補を優先し、揮発候補が実効の場合はログアウト後の値を表示するが、値の消失自体は防げない。
- レイヤーの導入状態はファイル存在ベースで、Vulkan ローダーが実際に読んだかは保証しない。`VK_ADD_IMPLICIT_LAYER_PATH` はゲーム側の探索パス差異を緩和するが、コンテナからパスが見えない場合は効果がない。
- `model_path` は絶対パスのみ受け付ける。既存の相対パス設定はゲーム側で解決されるため、GUI で保存する際に修正が必要になる。
- `$XDG_DATA_HOME` に `sed` の置換を壊す文字が含まれる環境ではレイヤー導入を拒否する。その場合は手動で XDG_DATA_HOME を変更するか、スクリプトを直接実行する必要がある。
- `ml` はモデル・DP4A・レイヤーのすべてが揃わないと FSR にフォールバックする。GUI は警告するが、ゲーム内の実際の経路は保証しない。
- キャプチャ環境変数とレイヤー有効化はゲーム起動時にしか読まれない。変更にはゲーム再起動が必要。
- 同梱インストーラは特定 commit のコピー。XRizer 側のレイヤー構成が変わったら更新が必要。
- X11 / Wayland のファイルダイアログは xdg-desktop-portal に依存する。失敗時はパスの直接入力で代替できる。
