# 149 号变更：移除全部 GitHub Actions / workflows 及其专用脚本

- 日期：2026-08-23
- 模块：.github / scripts / packages/cli（测试）
- 类型：结构清理（按用户决策）
- 关联：145 号（0.1.0 发布打包——release/desktop-build workflow 的最后演进）；本次为**终态决策**：此后本仓不再撰写任何 Actions / workflows 相关脚本与功能

## 一、决策

用户明确指示：移除所有 GitHub Actions 与 workflows 及对应脚本，**以后不再撰写**
Actions / workflows 相关的脚本和功能。CI / 发版 / 定时同步全部转本地手动执行。

## 二、删除清单

### 1. `.github/workflows/` 整目录（5 个文件）

| 文件 | 原职责 |
| --- | --- |
| `ci.yml` | Node 矩阵（ubuntu/windows × node 22/24）build+test、engine-rs clippy 零警告门禁、verify-pack（npm pack 无 workspace: 残留） |
| `release.yml` | tag v* → test → smoke → canary npm 发布 → verify-canary → 正式发布 → verify-release（全程用 `set-package-versions.mjs` 打版本号） |
| `desktop-build.yml` | 桌面壳三平台构建、engine bundle 打包/签名/上传、Tauri 发版 + updater latest.json |
| `desktop-e2e.yml` | 桌面 E2E 测试（macOS/Linux/Windows 矩阵） |
| `desktop-sync-regression.yml` | 每日 03:17 UTC 定时 merge 上游 Narcooo/inkos → 构建 → doctor + SSE 契约测，失败开 issue |

### 2. workflow 专用配套文件

- `.github/desktop-sync-failure.md`：sync workflow 失败时自动创建的 issue 模板
  （文件头注明「由 desktop-sync-regression workflow 自动创建」），随 workflow 一并失效。
- `scripts/set-package-versions.mjs`：唯一调用者是 release.yml 的 canary/正式发布
  版本号改写步骤，workflow 删除后无任何调用者。

### 3. 对应测试用例

- `packages/cli/src/__tests__/publish-package.test.ts` 的
  `rewrites workspace package versions for canary publishing`：直接 exec 已删除的
  `set-package-versions.mjs`，且其覆盖的 canary 发布流程仅存在于 release.yml。
  删除该用例；同文件其余 7 个用例（prepack/prepublishOnly/workspace 协议/npm pack
  完整性）与 workflow 无关，保留并验证通过（7/7 ✓）。

## 三、明确保留（有 workflow 之外的独立用途，非「对应脚本」）

| 文件 | 保留理由 |
| --- | --- |
| `scripts/desktop-package-engine.sh` | 虽被 desktop-build.yml 调用，但它是 engine 运行时组装核心：`src-tauri/tests/supervisor_integration.rs` 要求先跑它组装 `src-tauri/engine/`，本地 dev/集成测试必需 |
| `scripts/desktop-build-inkos.sh` | 本地构建引导（src-tauri/README.md 记载） |
| `scripts/desktop-gen-updater-key.sh` | 本地 updater 密钥生成（签名与分发指引） |
| `scripts/{prepare-package-for-publish,restore-package-json,verify-no-workspace-protocol}.mjs` | packages/*/package.json 的 prepack/postpack/prepublishOnly 钩子 + 根 npm scripts 仍引用 |
| `scripts/audit-semantic-patterns.mjs` | 根 package.json `audit:semantic-patterns` |
| `scripts/measure-sea-vs-bundle.sh`、`scripts/package-rust-engine.sh` | M4f 调研工具 / Rust 全量包本地发布工具（145 号） |
| `src-tauri/src/bin/{sign-bundle,publish-plugin}.rs` | Ed25519 签名与插件注册表生产端工具，有单元测试、属客户端验签工具链，本地手动发版可用 |
| `.github/ISSUE_TEMPLATE/`、`.github/pull_request_template.md` | issue/PR 模板，不属于 Actions/workflows |

## 四、连带更新

- `src-tauri/README.md`「零修改纪律」：桌面壳产物路径列表移除
  「新增 `.github/workflows/desktop-*.yml`」，并注明 workflows 已全量移除、此后不再撰写。

## 五、影响与后续

- GitHub 端 CI/CD/定时同步全部停止（远端 run 历史不受影响）。
- 发版（npm 包 / 桌面安装包 / engine bundle）与上游同步回归改为本地手动执行既有脚本
  （`package-rust-engine.sh`、`desktop-package-engine.sh`、`desktop-build-inkos.sh` 等，
  流程见 145 号与签名分发指引）。
- 验证：`packages/cli` publish-package 测试 7/7 通过；全仓 grep 无对已删文件的残留引用
  （历史归档文档与 SpecCoding 计划文档中的记述属历史记录，不改）。
