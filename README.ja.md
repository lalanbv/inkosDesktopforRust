<p align="center">
  <img src="assets/logo.svg" width="120" height="120" alt="InkOS Logo">
  <img src="assets/inkos-text.svg" width="240" height="65" alt="InkOS">
</p>

<h1 align="center">InkOS Desktop<br><sub>Tauri 2 ネイティブシェルとフル Rust エンジン上に構築された、物語創作 AI Agent デスクトップクライアント</sub></h1>

<p align="center">
  <a href="https://github.com/lalanbv/inkosDesktopforRust/releases"><img src="https://img.shields.io/badge/version-0.2.0-blue" alt="desktop v0.2.0"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-AGPL%20v3-blue.svg" alt="License: AGPL-3.0"></a>
  <a href="https://github.com/Narcooo/inkos"><img src="https://img.shields.io/badge/upstream-Narcooo%2Finkos-8B5CF6?logo=github" alt="upstream inkos repo"></a>
  <img src="https://img.shields.io/badge/engine-Rust%20%E7%9B%B4%E8%B5%B7%E5%8B%95%20%2B%20Node%20%E3%83%95%E3%82%A9%E3%83%BC%E3%83%AB%E3%83%90%E3%83%83%E3%82%AF-orange" alt="engine backend">
</p>

<p align="center">
  <a href="README.md">中文</a> | <a href="README.en.md">English</a> | 日本語
</p>

---

## これは何

[InkOS](https://github.com/Narcooo/inkos) は、物語創作と多言語翻訳に向けた AI Agent システムです。長編連載・独立短編・脚本・インタラクティブ影遊・オープンワールド・長文翻訳が、すべてひとつのワークベンチから始まります（プロダクト機能の詳細は上流 README を参照）。

**本リポジトリは InkOS のデスクトップクライアント分支**です（fork-and-own モノレポ）。ひとつのリポジトリに以下を含みます：

| 構成 | 内容 |
| --- | --- |
| `src-tauri/` | **Tauri 2 デスクトップシェル**：ウィンドウ / トレイ常駐 / エンジンプロセス監督 / Keychain 秘密鍵同期 / ネイティブ通知 / 自動アップデート / WASM プラグインシステム / マルチプロジェクト管理 |
| `engine-rs/` | **フル Rust エンジン**：`packages/core` の完全移植（16 ビジネスドメイン、`/api/v1/*` 107 エンドポイント + SSE + 静的面 + CORS）。単体の `inkos-engine-server` 実行ファイルとして提供 |
| `packages/{core,cli,studio}` | 上流 inkos v1.8.0。**デスクトップ UI の本体は `packages/studio`** にあり、四期 UI 改善はここで直接進化しています |
| `scripts/desktop-*` | ローカルビルド / エンジンパッケージ / リリーススクリプト（本プロジェクトに CI はなく、すべてローカルスクリプト化） |

デスクトップシェルは既定で **Rust エンジンを直接起動します（Node ランタイム不要）**。Rust バイナリが欠落しているか、明示的に `node` が設定された場合のみ、Node sidecar（`packages/cli/dist` + セルフコンテインド Node ブートストラップ、初回起動時にダウンロード）へフォールバックします。

<p align="center">
  <img src="assets/studio-dashboard.png" width="760" alt="InkOS Studio 創作入口">
</p>

## 現在の状態（2026-08）

| 項目 | 状態 |
| --- | --- |
| デスクトップシェル版 | v0.2.0（第 163 号、2026-08-24 にパッケージ化と三重 smoke テスト完了） |
| エンジンバックエンド | 既定で Rust 直起動（第 164 号の絞殺者パターン最終切替）。Node sidecar はフォールバック経路に降格 |
| エンジン更新チャネル | 実行中バックエンドに応じて分流。Rust / Node 両アセットチャネル + ヘルス事前チェック（第 166 号） |
| デスクトップ UI | 四期改善完了（第 150〜163 号）：コマンドパレット、タブ、集中モード、対比分割表示など。十次元レビュー 2.4 → 4.2 |
| CI | GitHub Actions は全廃（第 149 号）。リリースはローカルスクリプト + Release 手動アップロード |

## デスクトップ機能

### エンジンのデュアルバックエンド

| バックエンド | プロセス | ヘルスプローブ | 静的アセット |
| --- | --- | --- | --- |
| `rust`（既定） | `inkos-engine-server`（パッケージ内リソース、Node 不要） | `/api/v1/health` | `engine-rust/static/` |
| `node`（フォールバック） | `node engine/dist/index.js studio` | `/` | エンジンパッケージ内 SPA |

選択順序：環境変数 `INKOS_ENGINE_BACKEND`（rust\|node）> 設定パネルの「エンジンバックエンド」ドロップダウン（settings.html / TOML）> 既定の `rust`。Rust バイナリが見つからない場合は警告とともに自動で Node へフォールバックし、**起動はブロックされません**。診断コマンドは実行時に有効なバックエンドを表示し（`engine_backend: rust / node / unknown`）、設定上の意図と区別されます。

### システム統合

- **秘密鍵の安全保護**：システム Keychain（macOS Keychain / Windows Credential Manager / Linux Secret Service）とプロジェクトの `.inkos/secrets.json` を起動時に双方向同期 + ファイル監視ライトバック + 三重ループバック防止 + 0600 権限復元。Keychain が利用不能な場合は警告に降格し、使用をブロックしません
- **バックグラウンドでの通知**：SSE サイドバンドでエンジンイベント（`write:complete` / `book:created` など）を購読し、ウィンドウ非フォーカス時にネイティブ通知 + トレイバッジ
- **トレイ常駐**：ウィンドウを閉じるとトレイに格納。トレイ終了 / SIGINT / SIGTERM / Cmd+Q はすべて冪等クリーンアップ（プロセスグループ停止、リソース解放）を経由
- **自動アップデート**：tauri-plugin-updater + Ed25519 署名。アプリとエンジンの両方が検出→ダウンロード→インストール→失敗時ロールバックに対応し、エンジンは有効なバックエンドに応じて Rust / Node アセットを選択
- **ループバック強化**：macOS pf / Linux iptables / Windows netsh のプレースホルダ実装。特権なし起動時は自動降格して警告
- **可観測性**：tracing ログ、panic hook によるクラッシュ報告、診断コマンド（有効バックエンド・ポート・エンジンバージョン付き）

### デスクトップ化された Studio UI（第 150〜166 号の四期 + 収尾）

- **⌘K コマンドパレット**（39 コマンド）、**⌘P クイックオープン**（書物 / 章 / セッション / 設定の 4 種）、**⌘/** ショートカット一覧
- **タブマルチタスク**：VSCode 方式のプレビュー意味論、⌘数字切替、ピン留め、永続化、ディープリンク
- **四ゾーンワークベンチ**：アクティビティバー + サイドパネル（幅ドラッグ記憶 / ツリーフィルタ / ⌘B 折りたたみ）+ 右ドック + 下部タスクストリームとログ + ステータスバー（書物・章・文字数 / SSE / daemon 状態）
- **集中モード**、チャットの**メッセージ単位タイプライター**、**章の読み書き対比分割**（幅記憶 + 前/次章ジャンプ）
- ライト / ダーク / 自動テーマ（システム追従）+ 快適 / コンパクト密度切替。reduced-motion とキーボードアクセシビリティ（a11y）を全編でサポート
- 通知センターとタスク停止。macOS の統合タイトルバー / 信号灯 / ウィンドウ状態記憶 / メニューバー入口

### プラグインシステム

サンドボックス化された WASM プラグイン + 宣言的権限 + marketplace（詳細は [docs/plugin-system.md](docs/plugin-system.md)）。

## ソースから実行

### 環境要件

| ツール | バージョン | 備考 |
|------|------|------|
| Rust | stable | `rustup` でインストール |
| Node.js | 22+ | 上流 packages のビルドに必要 |
| **pnpm** | **10.x（必須）** | pnpm 11 は `pnpm.overrides` を読まなくなったため `--frozen-lockfile` は必ず失敗します。`corepack enable && corepack prepare pnpm@10.34.5 --activate` でバージョンを固定 |
| macOS | Xcode CLT | dev モード署名 |

### 手順

```bash
# 1. 上流成果物のビルド（pnpm install + core/cli/studio の全体ビルド）
./scripts/desktop-build-inkos.sh

# 2. Rust エンジンのビルド（dev ではシェルが engine-rs/target/ を直接探索）
cd engine-rs && cargo build --release

# 3. 任意：Rust エンジンリソースディレクトリの組立（.app パッケージ化に必須。純 dev ならスキップ可）
./scripts/desktop-package-rust-engine.sh

# 4. デスクトップシェルの起動
cd src-tauri && cargo run
```

期待される挙動：ウィンドウが開き picker ページ（プロジェクトの選択 / 新規 / 最近）が表示されます。プロジェクトを選ぶとエンジンが自動起動し、`http://127.0.0.1:<port>/` へ遷移します。dev モードでは Rust エンジンを `app_data/engine-rust` → `resource_dir/engine-rust` → `engine-rs/target/{release,debug}` の順に探索し、静的アセットは `packages/studio/dist` から取得します。

## インストーラ

[GitHub Releases](https://github.com/lalanbv/inkosDesktopforRust/releases) からダウンロード（v0.2.0 以降はローカルスクリプトでパッケージ化し手動アップロード）：

- macOS：`.dmg`（Apple Silicon / Intel）
- Windows：`.msi` / `.exe`
- Linux：`.deb` / `.AppImage`

システム要件：macOS 11+（Big Sur）/ Windows 10 1809+ / Ubuntu 20.04+、Debian 11+、Fedora 35+ など主要ディストリビューション。

インストール後はパッケージ内の Rust エンジンを既定で使用し、**Node.js は不要**です。Node バックエンドへフォールバックした場合のみ、初回起動時に Node ブートストラップをダウンロードします。

## ビルドとリリース（ローカルスクリプト、CI なし）

```bash
./scripts/desktop-build-inkos.sh                            # 1. フロントエンドと Node 成果物
INKOS_ENGINE_PROD=1 ./scripts/desktop-package-engine.sh    # 2. Node エンジンのセルフコンテインパッケージ
./scripts/desktop-package-rust-engine.sh                   # 3. Rust エンジンリソースディレクトリ
cd src-tauri && pnpm dlx @tauri-apps/cli@2 build           # 4. .app / .msi / .deb などのビルド
# macOS dmg：hdiutil UDZO + shasum -a 256。その後 git tag + Release ページで手動アップロード
```

その他のスクリプト：`package-rust-engine.sh`（単体配布用 Rust エンジン tarball、sha256 付随ファイル付き）、`desktop-gen-updater-key.sh`（updater 用 Ed25519 鍵ペア。公開鍵は tauri.conf.json に書き込み）。

## テスト

```bash
cd src-tauri && cargo test                 # 単体 + 結合 + doctest（モック、全緑）
cd src-tauri && cargo test -- --ignored    # 実 sidecar / keychain 結合テスト（実機のみ）
cd engine-rs && cargo test                 # エンジン単体 + golden 差分 + strangler duel
pnpm test && pnpm typecheck                # studio / core / cli の vitest と型検査
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html   # カバレッジ
```

## ドキュメント

- 📖 [ユーザーガイド](docs/USER_GUIDE.md) / 🚀 [クイックスタート](docs/QUICK_START.md) / 🔧 [トラブルシューティング](docs/TROUBLESHOOTING.md)
- ♿ [i18n とアクセシビリティ](docs/i18n-a11y.md) · 🧩 [プラグインシステム](docs/plugin-system.md) · 🔒 [セキュリティ監査](docs/security-audit.md) · ✍️ [署名調達](docs/signing-procurement.md) · 📦 [SEA 実現可能性](docs/sea-feasibility.md)
- 🛠 モジュール文書：[src-tauri/README.md](src-tauri/README.md)（デスクトップシェル）、[engine-rs/README.md](engine-rs/README.md)（エンジン移植の目標と規律）、[src-tauri/tests/README.md](src-tauri/tests/README.md)（テストガイド）
- 🗂 [变更记录文档/](变更记录文档/)（変更記録アーカイブ、連番）と [开发时SpecCoding'sPlan/](开发时SpecCoding'sPlan/)（設計計画。ディレクトリ名は中国語のまま）

> 注意：`docs/` 配下の一部文書は第 164 号「Rust エンジン既定直起動」切替より前のものです。USER_GUIDE / QUICK_START は第 172 号、TROUBLESHOOTING は第 179 号で整合・復査済み。疑問がある場合は本 README と最新の変更記録が正です。

## 開発規約

- **変更記録**：すべての変更は `变更记录文档/{YYYYMMDD}/{番号}_タイトル.md` にアーカイブし、番号は連続（現在 第 179 号）
- **workflow を書かない**：GitHub Actions は全廃済み（第 149 号）。以後 workflow および付随スクリプトを追加せず、リリースはローカルスクリプトで行う
- **上流との関係**：初期はマージコンフリクトを避けるため「上流ファイルの無変更」を堅持していましたが、第 150 号の UI 四期以降は `packages/studio` を本分支で直接進化させ（fork-and-own）、上流へマージバックしません
- デスクトップシェルのモジュールは単一責任を維持（src-tauri の各モジュールは 500 行未満の制約）。エンジン移植は 1:1 複刻 + golden 差分 + 契約 duel の規律に従います（詳細は engine-rs/README.md）

## 謝辞とライセンス

- 上流プロジェクト：[InkOS](https://github.com/Narcooo/inkos)（Narcooo）とそのコントリビューターの皆さん。InkOS の agent ランタイムは [pi](https://github.com/badlogic/pi-mono)（`@mariozechner/pi-ai` / `@mariozechner/pi-agent-core`）の上に構築されています
- 上流スポンサー：[ByteDance Volcano Engine](https://www.volcengine.com/activity/ai618?utm_source=OWO&utm_medium=devrel-1&utm_campaign=hw&utm_term=inkos&utm_content=hw) による InkOS へのスポンサーシップに感謝します（Volcano Ark Agent/Coding Plan。GLM-5.3、Kimi-K3、DeepSeek などのモデルに対応）
- ライセンス：[AGPL-3.0](LICENSE)。上流と同じです
