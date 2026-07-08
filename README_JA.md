# Run/Debug Configuration Manager

[English](README.md) · [한국어](README_KR.md) · **日本語**

Run/Debug 構成管理ツールです。
Rust と iced GUI フレームワークで開発されたクロスプラットフォームのデスクトップアプリケーションで、複数のプログラム実行構成を保存・管理できます。

![Preview](static/preview.gif)

## ダウンロード

プラットフォーム別の最新ビルドは [Releases ページ](https://github.com/devsepnine/debug_configuration/releases/latest) から取得するか、下記の直リンクをご利用ください:

| プラットフォーム | ファイル |
|---|---|
| Windows x64 | [`run_config_manager-windows-x64.msi`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-windows-x64.msi) |
| macOS (Apple Silicon) | [`run_config_manager-macos-arm64.dmg`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-macos-arm64.dmg) |
| macOS (Intel) | [`run_config_manager-macos-x64.dmg`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-macos-x64.dmg) |
| Linux x64 | [`run_config_manager-linux-x64.tar.gz`](https://github.com/devsepnine/debug_configuration/releases/latest/download/run_config_manager-linux-x64.tar.gz) |
| SHA-256 チェックサム | [`checksums.txt`](https://github.com/devsepnine/debug_configuration/releases/latest/download/checksums.txt) |

> macOS ユーザー向け: .app バンドルは ad-hoc 署名のみです (有料の Apple Developer ID は使用していません)。初回起動時の対応は [macOS — 初回起動](#macos--初回起動) セクションを参照してください。

## 主な機能

### 構成管理
- 実行構成の作成、編集、複製、削除
- 構成タイプの選択 (Application, Shell Script, Node, Kotlin, Compound)
- Compound タイプは複数の構成をまとめて一括実行
- コマンド、引数、作業ディレクトリの設定
- 環境変数の管理 (追加、編集、削除)
- ドラッグ&ドロップで構成の並び替え
- 構成のオープン/保存 (バージョン付き JSON 形式)

### Node プロジェクトサポート
- Node 関連コマンドのサポート (run, install, start, test, build など)
- Node Runtime の自動検出 (system PATH, nvm, nvm-windows)
- package.json の自動スキャンとスクリプトの解析
- パッケージマネージャーの選択または自動検出 (npm, yarn, pnpm, bun)
- 非同期初期化による高速起動

### Kotlin サポート
- `java` 経由で Kotlin/JVM アプリを実行 — Main class または JAR 起動モード
- VM options (例: `-Xmx2g`) とプログラム引数の設定
- JDK の自動検出 (JAVA_HOME, SDKMAN, macOS/Linux/Windows の標準的な場所) と手動指定

### 実行セッション管理
- 複数セッションの同時実行
- すべてのプラットフォームで本物の端末実行: macOS/Linux は PTY、Windows 10
  1809+ は ConPTY — プログラムが実コンソールを検出し、色・進捗バー・対話
  プロンプトが実シェル同様に動作 (ビューポート連動リサイズ; unix は
  `TERM=xterm-256color`)
- ライブ 1 行進捗バー (キャリッジリターンの再描画をその場でレンダリング;
  バックスペースで消去)
- セッションごとの stdin 入力バー (トグルボタンまたは Cmd+I): プロンプト応答や
  REPL 入力 — 端末がエコーを自然に処理、旧式 Windows pipe フォールバック
  (1809 未満) のみアプリがローカルエコー
- リアルタイム出力表示 (ANSI カラー対応; OSC/DCS 制御シーケンスは除去)
- セッションの再実行、停止、削除、ワークスペースから非表示
- 終了セッションのステータスバッジ (成功 / 失敗時の終了コード / 実行時間)
- ウィンドウが非アクティブな状態で実行完了したときのデスクトップ通知
- 一括操作: すべて停止、すべて再実行、失敗のみ再実行
- ワークスペース別の開閉状態を持つ永続セッションリスト
- ワークスペースタブシステム
  - 最低 1 つのワークスペースタブを保持
  - ワークスペースの追加、クローズ、リネーム
  - セッションリストから選択中のワークスペースに開く
- ペインベースのワークスペースレイアウト
  - iced pane grid によるペイン分割
  - ドラッグ&ドロップでペイン再配置
  - ペインのリサイズ
  - 複数ペイン時の最大化/復元

### 出力検索とエクスポート
- 出力検索 (Ctrl+F): 大文字小文字を無視する部分一致または正規表現
- マッチ行のハイライト、前後移動、マッチ件数表示
- マッチ行のみを表示するフィルターモード
- セッション出力全体をテキスト/ログファイルにエクスポート

### UI の特徴
- Catppuccin Mocha テーマ
- D2Coding フォント
- ターミナル風の出力
- カスタムウィンドウタイトルバーと丸みのあるウィンドウクロム
- ツールバー、セッション、ペインで統一されたアイコンボタン
- URL クリックでブラウザを開く
- クリップボードコピー対応
- 自動スクロール機能

## スクリーンショット

### 構成管理画面
![Configuration Management](static/config.png)

### 実行セッション画面
![Execution Sessions](static/session.png)

## インストールと実行

### 必要要件
- Rust 1.85 以上
- Cargo

### ビルド
```bash
cargo build --release
```

### 実行
```bash
cargo run --release
```

### Linux 実行時の依存関係
Linux ではいくつかのデスクトップサービスに実行時依存します。通常のデスクトップセッションには存在しますが、最小構成/ヘッドレス環境では欠けていることがあり、その場合は該当機能が静かに無効化されます:

- **ファイルダイアログ**（開く / 保存 / 出力エクスポート）は D-Bus 経由の XDG Desktop Portal を使用します — `xdg-desktop-portal` とバックエンド（`xdg-desktop-portal-gtk`、`-kde`、`-hyprland` など）をインストールして起動してください。
- **デスクトップ通知** には `org.freedesktop.Notifications` を実装する通知デーモンが必要です（GNOME/KDE は標準提供。なければ `dunst` / `mako`）。
- **URL を開く** には `xdg-utils` の `xdg-open` を使用します。
- **描画** は wgpu（Vulkan/GL）を使用します。GPU ドライバー（Mesa）の動作を推奨します（tiny-skia ソフトウェア描画がフォールバック）。

ソースからビルドする際のシステムライブラリは CI ワークフローに記載: `libfontconfig1-dev`、`libwayland-dev`、`libx11-xcb-dev`、`libxkbcommon-dev`、`pkg-config`。

### プラットフォームチェック
本プロジェクトは Windows、macOS、Linux での動作を目標としています。

以下のターゲットでコンパイルチェックを通過しています:
```bash
cargo check --target x86_64-pc-windows-msvc
cargo check --target x86_64-unknown-linux-gnu
cargo check --target x86_64-apple-darwin
cargo check --target aarch64-apple-darwin
```

透過の丸ウィンドウ角のような OS コンポジターに依存する見え方は、各プラットフォームでの実機確認が必要です。

### 自動リリース
GitHub Actions は pull request と `develop`、`release/**` ブランチへの push で CI 検証を実行します。
リリース成果物は 4 つのプラットフォーム向けにビルドされます:
- Windows x64 — MSI インストーラー
- Linux x64 — desktop エントリ、アイコン、インストールスクリプトを含む tarball
- macOS x64 — DMG ディスクイメージ
- macOS arm64 — DMG ディスクイメージ

Linux のリリースアーカイブをインストール:
```bash
tar -xzf run_config_manager-linux-x64.tar.gz
cd run_config_manager-linux-x64
./install-linux.sh
```

#### 自動リリース (推奨)
`release/vX.Y.Z` ブランチを push すると自動ビルドと draft GitHub Release が作成されます:
```bash
git checkout -b release/v0.2.0 develop
# Cargo.toml のバージョン更新、最後の調整
git push -u origin release/v0.2.0
```
ワークフローはバージョン形式 (`vX.Y.Z` または `vX.Y.Z-suffix`) を検証し、4 つのプラットフォームをビルドして `checksums.txt` とともに draft リリースにアップロードします。
GitHub Releases ページで draft を確認し、**Publish release** ボタンで公開してください。タグは publish 時にブランチの tip に作成されます。
その後 `release/v0.2.0` を `develop` にマージしてください。

#### 手動リリース (フォールバック)
タグが既に存在する場合や、ブランチフローを経由しない場合は、タグを先に push して GitHub Actions タブから `Release` ワークフローを手動実行してください:
```bash
git tag v0.2.0 <commit>
git push origin v0.2.0
```

両方の経路とも、同じタグのリリースが既にあれば停止します。
既存リリースの checksum を再生成するときだけ `Checksum` ワークフローを実行してください。

### macOS — 初回起動

.app バンドルは ad-hoc 署名のみです (有料の Apple Developer ID は使用していません)。初回起動時に macOS が以下のいずれかを表示する場合があります:
- `Apple could not verify "RunConfigManager" is free of malware...` (macOS 14+)
- `"RunConfigManager" is damaged and can't be opened` (quarantine 属性が有効な場合)

![macOS Gatekeeper dialog](static/done.png)

許可する 2 通りの方法:

**方法 1 — システム設定 (推奨)**
1. ダイアログで `Done` をクリックして閉じる。
2. **システム設定 → プライバシーとセキュリティ** を開く。
3. セキュリティセクションに `"RunConfigManager" was blocked` メッセージと **Open Anyway** ボタンが表示されているはず。

   ![Privacy & Security – Open Anyway](static/openanyway.png)

4. **Open Anyway** をクリックし、Touch ID またはパスワードで認証。
5. RunConfigManager を再度起動 → 以後は通常起動できます。

**方法 2 — ターミナル (一発で)**
```bash
xattr -dr com.apple.quarantine /Applications/RunConfigManager.app
open /Applications/RunConfigManager.app
```

`com.apple.quarantine` 属性を削除して Gatekeeper の検査自体をバイパスし、ダイアログなしで直接起動できます。

## 使い方

### 1. 構成の作成
1. `Configurations` タブで `Add` ボタンをクリック
2. 構成名、タイプ、コマンドなどを設定
3. 必要に応じて環境変数を追加
4. `Save` ボタンで保存

### 2. プログラムの実行
1. 実行する構成を選択
2. `Run` ボタンまたは構成リストの再生ボタンをクリック
3. `Sessions` タブに自動切り替え、実行結果を確認

### 3. ペイン管理
- **セッションを開く**: 左のセッションリストでセッションをクリックして、選択中のワークスペースで開く
- **セッションを非表示**: ペインのクローズボタンで、実行中のセッションは保持したままワークスペースから外す
- **セッションの停止/削除**: セッションリストのアクションボタンで停止または削除
- **ペイン移動**: ペインヘッダーをドラッグして再配置
- **ペインのサイズ変更**: ペイン境界をドラッグしてリサイズ
- **ペイン最大化**: ワークスペースに 2 つ以上ペインがある場合、最大化ボタンを使用

### 4. 構成のオープン/保存
- **Open**: JSON 構成ファイルを開いて現在の構成を置き換える
- **Save**: 現在のファイルに保存、パスがない場合は保存先を選択

## データ保存場所

構成ファイルは OS 別の設定ディレクトリに自動保存されます:
- **Linux**: `~/.config/run_config_manager/configs.json`
- **macOS**: `~/Library/Application Support/run_config_manager/configs.json`
- **Windows**: `%APPDATA%\run_config_manager\configs.json`

## 既知の制限

- **フルスクリーン TUI アプリは対象外**: 端末はスクリーングリッドではなく
  行スクロールバックのレンダラーのため、`vim`、`htop`、`less` などの
  alt-screen プログラムは表示が崩れます (アプリ自体は正常 — Stop で復旧)。
  `input()`、`read`、REPL などの行ベースの対話プロンプトは動作します。タブ文字は
  リテラル表示です (カラム展開なし)。
- **Windows の特記事項**: ConPTY はレンダリング済みの VT ストリームを渡し、
  アプリはそれを行スクロールバックとして表示します — カーソル中心の編集は行単位
  で近似されます (バックスペース消去、キャリッジリターンで行書き換え)。Stop は
  `taskkill /T /F` の強制終了で、終了コードは通常 1 と報告されます (まれに
  259/STILL_ACTIVE が見えることがあります)。子プロセス終了後 ~300ms 沈黙した
  孫プロセスの以降の出力は打ち切られます (unix は実際の EOF まで維持)。ConPTY の
  ない Windows 10 1809 未満は pipe にフォールバック — 色/進捗バーなし、ローカル
  エコー、セッションにバナー表示。予期しない PowerShell プロンプトの待機
  (ハングのように見える — Stop で終了) の注意はこのフォールバックのみに該当します。
- macOS/Linux では `sh -l` + 本物の tty のため、シェルプロファイルのバナーが
  セッション出力に現れることがあります。

## 終了処理

アプリケーション終了時に実行中の全プロセスを自動的にクリーンアップします:
1. Ctrl+C またはウィンドウクローズで Drop trait が実行
2. Unix: セッションごとのプロセスグループに SIGTERM → 2 秒の猶予 → 残存に SIGKILL
3. Windows: 直ちに `taskkill /T /F` — ConPTY には graceful なシグナル経路が
   ないためツリーを強制終了

## ライセンス

本プロジェクトは GNU General Public License v3.0 で配布されています。詳細は [LICENSE](LICENSE) を参照してください。
