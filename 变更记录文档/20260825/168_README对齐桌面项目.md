# 168 · 根 README 对齐桌面客户端分支现状

- 日期：2026-08-25
- 模块：根 `README.md`（文档）
- 类型：文档纠偏
- 关联：145 号（0.1.0 发布）、149 号（移除 workflow）、150~163 号（UI 四期 + v0.2.0）、164~167 号（Rust 引擎直切与补齐）

## 一、背景

根 README.md 长期沿用上游 inkos 的产品 README（npm 安装、CLI 命令参考、TUI/网页版营销区块、npm 徽章等），仅在中段夹了一小节「桌面应用（InkOS Desktop）」。与本仓库现状严重不符：

| 不符点 | 实际情况 |
| --- | --- |
| npm 徽章与 `npm i -g @actalk/inkos` 安装指引 | 本仓库根 `package.json` 为 `private: true`，不发 npm；安装走 GitHub Releases 桌面包 |
| 「Engine 自动下载（Node）」叙事 | 164 号起默认 Rust 引擎直启，零 Node 运行，Node 仅回退 |
| 版本叙事 v1.8.0（上游） | 桌面壳 v0.2.0（tauri.conf.json / Cargo.toml），引擎 1.8.0 / Rust 引擎包 0.1.0 |
| 未提及 engine-rs、双后端、updater 双通道、四期 UI 成果 | 这些是 150~167 号的主体交付 |
| 「三平台 CI 门禁」表述 | 149 号已全量移除 GitHub Actions，发版本地脚本化 |

## 二、改动（仅根 README.md，全量重写）

新结构：这是什么（fork-and-own mono-repo 组成表）→ 当前状态表 → 桌面端能力（引擎双后端 / 系统集成 / 四期 UI / 插件系统）→ 从源码运行（环境要求含 pnpm 10.x 锁版本 + 四步构建）→ 安装包下载 → 构建与发布（163 号本地脚本链）→ 测试 → 文档索引 → 开发约定（变更记录归档 / 不写 workflow / 上游关系演变）→ 致谢与许可证。

事实核对来源：tauri.conf.json（0.2.0 / updater endpoint 指向 lalanbv fork）、src-tauri 与 engine-rs 的 Cargo.toml、164/166 号变更记录（后端选择序 / BundleFlavor）、163 号（v0.2.0 打包链与十维复评）、149 号（无 CI）、scripts/desktop-* 脚本头注释。

处理决策：

1. **删**：上游营销区块（网页版 / Kimi 赞助 / 火花数据 / 微信群）、npm/Star History/Repobeats/Contributors 徽章区块、CLI 命令参考全表、LLM 配置三方式详解——均为上游产品内容，指向 Narcooo/inkos 上游 README 即可
2. **留并更新**：logo 头图、studio-dashboard 截图、AGPL 许可证、pi 致谢（上游依赖仍在 mono-repo 内）
3. **新增**：README.en.md / README.ja.md 仍为上游内容的显式提示，避免语言切换误导
4. **docs 滞后提示**：docs/USER_GUIDE（v0.4.0-alpha）与 QUICK_START（Node 首启下载叙事）早于 164 号切换，README 中加了「以本 README 与最新变更记录为准」的注意——docs 本身的更新留后续单独变更，不在本轮扩散范围

## 三、自查

- 徽章改静态版本徽章（0.2.0），不依赖尚未推送的 GitHub Release
- 引擎双后端表、选择序、诊断回显、updater 双通道均与 164/166 号记录逐条对照
- 源码运行四步与 scripts 头注释一致（desktop-package-rust-engine.sh 标注「打包必需 / 纯 dev 可跳过」）
- 开发约定段与 AutoRecordChanges / 不写 workflow / 150 号起直接演进 packages/studio 的实际历史一致
