<p align="center">
  <img src="assets/logo.svg" width="120" height="120" alt="InkOS Logo">
  <img src="assets/inkos-text.svg" width="240" height="65" alt="InkOS">
</p>

<h1 align="center">InkOS Desktop<br><sub>A story-creation AI Agent desktop client built on a Tauri 2 native shell and a full-Rust engine</sub></h1>

<p align="center">
  <a href="https://github.com/lalanbv/inkosDesktopforRust/releases"><img src="https://img.shields.io/badge/version-0.2.0-blue" alt="desktop v0.2.0"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-AGPL%20v3-blue.svg" alt="License: AGPL-3.0"></a>
  <a href="https://github.com/Narcooo/inkos"><img src="https://img.shields.io/badge/upstream-Narcooo%2Finkos-8B5CF6?logo=github" alt="upstream inkos repo"></a>
  <img src="https://img.shields.io/badge/engine-Rust%20native%20%2B%20Node%20fallback-orange" alt="engine backend">
</p>

<p align="center">
  <a href="README.md">中文</a> | English | <a href="README.ja.md">日本語</a>
</p>

---

## What is this

[InkOS](https://github.com/Narcooo/inkos) is an AI Agent system for story creation and multilingual translation: long-form serials, standalone short fiction, scripts, interactive film/games, open worlds, and long-form translation — all starting from a single workbench (product capabilities are documented in the upstream README).

**This repository is InkOS's desktop-client branch** (a fork-and-own mono-repo) containing, in one place:

| Component | Contents |
| --- | --- |
| `src-tauri/` | **Tauri 2 desktop shell**: window / tray keep-alive / engine process supervision / Keychain secret sync / native notifications / auto-update / WASM plugin system / multi-project management |
| `engine-rs/` | **Full Rust engine**: a complete port of `packages/core` (16 business domains, `/api/v1/*` 107 endpoints + SSE + static assets + CORS), shipped as the standalone `inkos-engine-server` executable |
| `packages/{core,cli,studio}` | Upstream inkos v1.8.0; **the desktop UI lives in `packages/studio`**, where the four-phase UI overhaul evolves directly |
| `scripts/desktop-*` | Local build / engine packaging / release scripts (this project has no CI; everything is local scripting) |

By default the desktop shell **launches the Rust engine directly (zero Node runtime)**; if the Rust binary is missing or the backend is explicitly set to `node`, it falls back to a Node sidecar (`packages/cli/dist` + a self-contained Node bootstrap, downloaded on first launch).

<p align="center">
  <img src="assets/studio-dashboard.png" width="760" alt="InkOS Studio creation entry">
</p>

## Current status (2026-08)

| Item | Status |
| --- | --- |
| Desktop shell version | v0.2.0 (change #163, packaged and triple-smoke-tested on 2026-08-24) |
| Engine backend | Rust direct-launch by default (#164, the final strangler switch); Node sidecar demoted to fallback |
| Engine update channel | Split by the effective backend: parallel Rust / Node asset channels with health pre-checks (#166) |
| Desktop UI | Four-phase overhaul complete (#150–163): command palette, tabs, focus mode, split-pane reading, ten-dimension review score 2.4 → 4.2 |
| CI | GitHub Actions fully removed (#149); releases are produced by local scripts and manually uploaded to Releases |

## Desktop capabilities

### Dual engine backends

| Backend | Process | Health probe | Static assets |
| --- | --- | --- | --- |
| `rust` (default) | `inkos-engine-server` (bundled resource, zero Node) | `/api/v1/health` | `engine-rust/static/` |
| `node` (fallback) | `node engine/dist/index.js studio` | `/` | SPA inside the engine package |

Selection order: env var `INKOS_ENGINE_BACKEND` (rust\|node) > the "Engine backend" dropdown in the settings panel (settings.html / TOML) > default `rust`. If the Rust binary is missing, the shell automatically falls back to Node with a warning and **startup is never blocked**; diagnostics echo the effective backend at runtime (`engine_backend: rust / node / unknown`), kept distinct from the configured intent.

### System integration

- **Secret safety**: bidirectional startup sync between the system keychain (macOS Keychain / Windows Credential Manager / Linux Secret Service) and the project `.inkos/secrets.json`, with file-watch write-back, triple loop-back protection, and 0600 permission restoration; if the keychain is unavailable it degrades to a warning without blocking use
- **Background without interruption**: SSE side-band subscription to engine events (`write:complete` / `book:created` etc.) drives native notifications + tray badge when the window loses focus
- **Tray keep-alive**: closing the window hides to tray; tray quit / SIGINT / SIGTERM / Cmd+Q all go through idempotent cleanup (process-group kill, resource release)
- **Auto-update**: tauri-plugin-updater + Ed25519 signatures; both app and engine support detect → download → install → rollback-on-failure, with engine assets chosen by the effective backend (Rust / Node)
- **Loopback hardening**: macOS pf / Linux iptables / Windows netsh placeholder implementations that auto-degrade with a warning when started without privileges
- **Observability**: tracing logs, panic-hook crash reporting, diagnostic command (effective backend, port, engine version)

### Desktop-grade Studio UI (four phases #150–166 + finishing)

- **⌘K command palette** (39 commands), **⌘P quick open** (books / chapters / sessions / settings), **⌘/** shortcut cheatsheet
- **Tabs**: VSCode-style preview semantics, ⌘number switching, pinning, persistence, deep links
- **Four-zone workbench**: activity bar + side panel (width memory / tree filter / ⌘B collapse) + right dock + bottom task stream & logs + status bar (book · chapter · word count / SSE / daemon state)
- **Focus mode**, chat **message-level typewriter**, **chapter read/write split pane** (width memory + prev/next chapter navigation)
- Light / dark / auto themes following the system + comfortable / compact density; reduced-motion and keyboard accessibility (a11y) throughout
- Notification center and task stopping; macOS unified title bar / traffic lights / window-state memory / menu-bar entry

### Plugin system

Sandboxed WASM plugins + declarative permissions + marketplace (see [docs/plugin-system.md](docs/plugin-system.md)).

## Run from source

### Requirements

| Tool | Version | Notes |
|------|------|------|
| Rust | stable | install via `rustup` |
| Node.js | 22+ | needed to build the upstream packages |
| **pnpm** | **10.x (required)** | pnpm 11 no longer reads `pnpm.overrides`, so `--frozen-lockfile` always fails; pin with `corepack enable && corepack prepare pnpm@10.34.5 --activate` |
| macOS | Xcode CLT | dev-mode signing |

### Steps

```bash
# 1. Build upstream artifacts (pnpm install + full core/cli/studio build)
./scripts/desktop-build-inkos.sh

# 2. Build the Rust engine (in dev the shell probes engine-rs/target/ directly)
cd engine-rs && cargo build --release

# 3. Optional: assemble the Rust engine resource directory (required for packaging the .app; skippable in pure dev)
./scripts/desktop-package-rust-engine.sh

# 4. Launch the desktop shell
cd src-tauri && cargo run
```

Expected: a window opens on the picker page (choose / create / recent projects); after a project is picked the engine starts automatically and navigates to `http://127.0.0.1:<port>/`. In dev mode the Rust engine is probed in the order `app_data/engine-rust` → `resource_dir/engine-rust` → `engine-rs/target/{release,debug}`; static assets come from `packages/studio/dist`.

## Installers

Download from [GitHub Releases](https://github.com/lalanbv/inkosDesktopforRust/releases) (from v0.2.0, packaged by local scripts and uploaded manually):

- macOS: `.dmg` (Apple Silicon / Intel)
- Windows: `.msi` / `.exe`
- Linux: `.deb` / `.AppImage`

System requirements: macOS 11+ (Big Sur) / Windows 10 1809+ / Ubuntu 20.04+, Debian 11+, Fedora 35+ and other mainstream distributions.

After install the bundled Rust engine is used by default — **no Node.js required**; the Node bootstrap is only downloaded when falling back to the Node backend.

## Build & release (local scripts, no CI)

```bash
./scripts/desktop-build-inkos.sh                            # 1. Frontend + Node artifacts
INKOS_ENGINE_PROD=1 ./scripts/desktop-package-engine.sh    # 2. Self-contained Node engine package
./scripts/desktop-package-rust-engine.sh                   # 3. Rust engine resource directory
cd src-tauri && pnpm dlx @tauri-apps/cli@2 build           # 4. Build .app / .msi / .deb etc.
# macOS dmg: hdiutil UDZO + shasum -a 256; then git tag + manual upload on the Releases page
```

Other scripts: `package-rust-engine.sh` (standalone Rust-engine tarball release with a sha256 sidecar) and `desktop-gen-updater-key.sh` (updater Ed25519 keypair; the pubkey is written into tauri.conf.json).

## Testing

```bash
cd src-tauri && cargo test                 # unit + integration + doctests (mocked, all green)
cd src-tauri && cargo test -- --ignored    # real sidecar / keychain integration tests (real machine only)
cd engine-rs && cargo test                 # engine units + golden differentials + strangler duels
pnpm test && pnpm typecheck                # vitest and type checks for studio / core / cli
cd src-tauri && cargo llvm-cov --workspace --html --output-dir target/llvm-cov/html   # coverage
```

## Documentation

- 📖 [User guide](docs/USER_GUIDE.md) / 🚀 [Quick start](docs/QUICK_START.md) / 🔧 [Troubleshooting](docs/TROUBLESHOOTING.md)
- ♿ [i18n & accessibility](docs/i18n-a11y.md) · 🧩 [Plugin system](docs/plugin-system.md) · 🔒 [Security audit](docs/security-audit.md) · ✍️ [Signing procurement](docs/signing-procurement.md) · 📦 [SEA feasibility](docs/sea-feasibility.md)
- 🛠 Module docs: [src-tauri/README.md](src-tauri/README.md) (desktop shell), [engine-rs/README.md](engine-rs/README.md) (engine porting goals & discipline), [src-tauri/tests/README.md](src-tauri/tests/README.md) (testing guide)
- 🗂 [变更记录文档/](变更记录文档/) (change log archive, sequential numbering) and [开发时SpecCoding'sPlan/](开发时SpecCoding'sPlan/) (design & planning; directory names kept in Chinese)

> Note: some documents under `docs/` (e.g. TROUBLESHOOTING) predate the #164 "Rust engine direct-launch" switch and may be stale; USER_GUIDE / QUICK_START were aligned in #172. When in doubt, this README and the latest change records are authoritative.

## Development conventions

- **Change log**: every change is archived under `变更记录文档/{YYYYMMDD}/{number}_title.md` with continuous numbering (currently at #177)
- **No workflows**: GitHub Actions were fully removed (#149); no new workflows or companion scripts will be added — releases go through local scripts
- **Relationship with upstream**: early on we kept a strict "zero modification of upstream files" policy to reduce merge conflicts; from #150 (the four-phase UI overhaul) `packages/studio` evolves directly on this branch (fork-and-own) and is no longer merged back upstream
- The desktop-shell modules keep single responsibility (a < 500-line bound per src-tauri module); engine porting follows 1:1 replication + golden differentials + contract duels (see engine-rs/README.md)

## Acknowledgements & license

- Upstream project: [InkOS](https://github.com/Narcooo/inkos) (Narcooo) and its contributors; InkOS's agent runtime is built on [pi](https://github.com/badlogic/pi-mono) (`@mariozechner/pi-ai` / `@mariozechner/pi-agent-core`)
- Upstream sponsor: thanks to [ByteDance Volcano Engine](https://www.volcengine.com/activity/ai618?utm_source=OWO&utm_medium=devrel-1&utm_campaign=hw&utm_term=inkos&utm_content=hw) for sponsoring InkOS (Volcano Ark Agent/Coding Plan with GLM-5.3, Kimi-K3, DeepSeek and other models)
- License: [AGPL-3.0](LICENSE), same as upstream
