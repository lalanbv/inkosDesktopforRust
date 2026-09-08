# 227 号：observability/crash 报告面 + secrets 管理面复核——secrets.json 0600 权限修复

- 日期：2026-09-09
- 分支：develop
- 关联：221 号（secrets 原子写——本批补权限维度）、src-tauri M3b（`atomic_write_0600` 同语义先例）
- 推送核验：origin/develop 仍停 6bc65564——**201–226 共 26 提交待推送**；本批次后本地领先 27。两项默认值无新答复。

## 一、observability/crash 报告面（src-tauri observability，533 行）

| 审计点 | 结论 |
|---|---|
| crash dump 保留策略 | ✓ LRU 保留最近 10 个（`MAX_CRASH_DUMPS`，按修改时间删最旧）——不会无限堆积；有单测（13 个 dump 留 10） |
| panic hook | ✓ init_panic_hook 落盘 crash_dir（app data/crashes，fallback temp），路径解析有降级 |
| diagnostics | ✓ M4a 诊断 JSON（版本/平台/路径/manifest/最近崩溃）供 UI 呈现 |

零缺口。

## 二、secrets 管理面

| 面 | 结论 |
|---|---|
| src-tauri secrets.json（app 数据目录） | ✓ M3b 已是 0600 + 原子替换（`atomic_write_0600`，projects.json 复用同语义） |
| legacy service id 迁移 | ✓ siliconflow→siliconcloud（new id 不存在才复制+删旧） |
| **engine `.inkos/secrets.json` 权限** | **缺口（已修）**：`save_secrets` 无权限设置——`fs::write`/`rename` 默认 0644（umask 影响），文件含**明文 API key**，同机其他用户可读；项目目录若在共享位置/云盘则泄露面更大 |

**修复**：`save_secrets` 在 rename 前 `set_permissions(0o600)`（Unix cfg；Windows 无权限位跳过）——对齐 src-tauri M3b 同语义；rename 前 chmod 消除「写完到改名之间的可读窗口」。单测：mode & 0o777 == 0o600 断言。

## 三、验证

- engine：lib **1311**（+1 权限断言）、集成 **196**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。
- TS 零改动。注：TS 写 secrets 同为默认权限——同 221 号逻辑，Rust 侧先行消除（默认引擎），TS 随绞杀者淘汰。

## 四、遗留

无新增。质量面复核候选继续：updater 双通道（166号）边界、plugin 管理面。
