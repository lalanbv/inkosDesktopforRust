# 1597 static 快照漂移重烤 + testgate 当前 HEAD 闭环轮（2026-11-20）

## 主靶面①：打包快照链盲区坐实并修复（desktop-static-bake-chain 记忆直用）

快照核查（秒级 shasum 对比）：`src-tauri/engine-rust/static/index.html` 哈希≠`packages/studio/dist/index.html`（快照 **Nov 16 00:45** vs dist **Nov 20 04:50**——快照落后 4 天）+assets 477 vs 421。风险面=正式打包烤入陈旧前端（gitignore 看不到漂移=零信号盲区）。

修复：`desktop-package-rust-engine.sh` 幂等重跑（脚本形态=**rm -rf 清旧全量重建**，非「rsync 无 --delete」旧形态——记忆纠偏：当前 dev 两处烤入面均无残留风险；Node 腿 SPA=运行时经 node_modules 解析 packages/studio/dist **活源**非快照，1596 Node 腿 SPA 200 的真因）。release 重编 51s+组装 OK。

复验全一致：index.html 哈希一致+assets **421==421**+manifest（engine_sha256=b9c25008…/git_commit=9343efe3/built_at 刷新）+engine-rust 不进 git（gitignore）。

**用户侧待办「打包三脚本重烤」实质完成**：desktop-package-rust-engine.sh（本轮重烤）+desktop-package-engine.sh（1593 sweep 自动组装，engine/dist 11-19 21:18）+desktop-build-inkos.sh（dist 04:50 真实构建非 duel 桩）——三脚本当前全部新鲜；剩余=真实 Tauri build 打包+安装包验证（用户侧 GUI 形态）。

## 主靶面②：testgate 2731 当前 HEAD（9343efe3）全量闭环

1596 动了 supervisor.rs（纯注释）——1590 证据链严格延伸至 1595 为止，当前 HEAD diff 非空集。本轮 `pnpm cargo:testgate`：**2731 passed**（engine-rs 58 目标 2131+src-tauri 26 目标 600）全绿，含 duel 真跑（INKOS_DUEL=1 强制）+audit:rust 必红自检活（9/1 夹具检出）。至此全部验证面在当前 HEAD 有同日/当周证据。

## 上游核查

`git fetch upstream` 零落后（0 新提交，二次确认）。

## 本轮零源码变更（重烤面=gitignore 资源，不进提交）

## 后续

- 下号自 1598 起；用户侧待办收窄：① Fork 图形端推送（955-1597）② 真实 Tauri build 打包+安装包验证（三脚本已新鲜，直接走打包即可）。
