# 变更记录：M3f 签名 + OSS 分发指引

| 项 | 值 |
|---|---|
| 日期 | 2026-08-06 |
| 类型 | docs（M3 第 6 子里程碑） |
| 范围 | 签名 secrets 配置 + OSS 用户分发指引 + 发版清单 |
| 设计 | `03_M3设计/M3总体设计.md` §4 M3f |

## 交付

签名**逻辑**（ad-hoc 默认 + CI secrets gating）已在 M3e `desktop-build.yml`（tauri-action）。
M3f 补齐**运维设置**（secrets）+ **用户分发**指引。

### 新增 `开发时SpecCoding'sPlan/inkosDesktop/04_运维与分发/签名与分发指引.md`
- **当前策略**：GitHub OSS，无 Apple Developer ID → macOS ad-hoc + 用户 `xattr -dr`，
  Windows SmartScreen 引导，Linux AppImage chmod。表格化三平台。
- **未来升级路径**：CI secrets 配齐 `APPLE_*` / Authenticode → tauri-action 自动升级签名+公证，
  零代码改动；未配自动回退 ad-hoc。
- **CI Secrets 配置**：
  - shell updater Ed25519（`TAURI_SIGNING_PRIVATE_KEY`，必配；keypair 经 `scripts/desktop-gen-updater-key.sh`）。
  - macOS Developer ID + 公证（`APPLE_*`，可选未来）。
  - Windows Authenticode（`TAURI_SIGNING_*`，可选未来）。
- **用户安装指引**（发版 notes 引用）：macOS `xattr -dr com.apple.quarantine`、
  Windows SmartScreen「仍要运行」、Linux `chmod +x`。
- **发版清单**（release checklist）：版本三处一致、keypair 配对、tag 触发 desktop-build、release notes 附指引。
- **安全说明**：secrets 仅 CI；engine SHA256 + shell Ed25519 强校验；用户主动触发更新（无静默安装）。
- **ad-hoc 本地验证**（`codesign -dv`）。

## 兼容性 / 扩展性 / 安全

- **兼容**：三平台均有可行分发路径（无证书也能用，配证书即升级）。
- **扩展**：secrets gating 未来就绪（零代码升级签名）。
- **安全**：私钥仅 CI；强校验；用户主动更新。
- **零修改**：新文档（不碰 inkos README）。

## 验证

- 签名逻辑（M3e workflow）YAML 合法 + tauri-action 标准用法。
- 文档发版清单可执行（与 M3e desktop-build.yml 步骤对齐）。

## 下一步

M3 总验收：全量 test + clippy + 审查 + tag v0.3.0-m3 + 归档。
