---
title: "[自动] 上游同步回归失败"
labels: ["sync-regression", "bug"]
---

## 上游同步回归失败

由 `desktop-sync-regression` workflow 自动创建。需人工介入。

### 常见原因

- **merge 冲突**：`git merge upstream/master` 有冲突（根文件改动重叠）→ 人工解决。
- **构建失败**：`pnpm build` 失败（上游 breaking change 影响构建）。
- **inkos doctor 失败**：上游结构性问题。
- **SSE 契约测失败**：上游 `server.ts` 的 `broadcast()` 事件集合变化，导致本仓 observer
  `default_table` 不再 ⊆ inkos broadcast（架构 §6.2 容错：observer 忽略未知事件，不崩溃；
  但契约测提醒需更新 `default_table` 以恢复原生通知路由）。
- **Rust 测试失败**：桌面壳测试回归。

### 处置

1. 查 workflow run 日志定位失败步骤。
2. SSE 契约测失败 → 更新 `src-tauri/src/observer/router.rs` 的 `default_table`（基于
   `grep broadcast( packages/studio/src/api/server.ts`）。
3. merge 冲突 → 本地 `git merge upstream/master` 解决 + 推送。
4. 解决后关闭本 issue，下次定时跑验证。

### run

{{ env.GITHUB_SERVER_URL }}/{{ env.GITHUB_REPOSITORY }}/actions/runs/{{ env.GITHUB_RUN_ID }}
