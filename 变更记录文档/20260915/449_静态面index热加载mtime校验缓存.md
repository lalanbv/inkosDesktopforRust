# 449 号：静态面 index.html 热加载——mtime 校验缓存（445 备案修复）

- **日期**：2026-09-15
- **类型**:fix(engine) + perf —— 开发/运维体验缺陷修复（保留 0GC 零拷贝语义）
- **关联**：124 号（静态面对照移植）、170 号 W-B1（Bytes 零拷贝）、445 号（重建 dist 后白屏备案）
- **提交**：engine-rs/src/server/static_routes.rs + 本记录

## 1. 缺陷（445 号备案的机制定位）

445 号走查实测「重建 dist 后浏览器白屏，须重启引擎才恢复」。机制定位：`StaticFace.index` 在 `with_static_face` **启动期一次性读入** `Bytes` 并驻留共享态，SPA 回退永远服务该快照——引擎运行期间 dist 的任何变化（重建/回滚）均不可见。

## 2. 修复（mtime 校验缓存，零拷贝语义保留）

- `StaticFace.index` 从 `Option<Bytes>` 改为 `RwLock<CachedIndex { mtime, body }>`；
- SPA 回退每请求一次 `stat(index.html)`：mtime 未变 → 缓存 `Bytes` 克隆（引用计数递增，W-B1 语义不变）；变化/首次 → 重读并刷新缓存（写锁内二次校验防竞态重读）；
- 附带改进：**启动时 index 缺失**（旧实现永久 404）→ 事后补建也会被拾取。

## 3. 测试与验证

- 新增 2 单测：`index_hot_reload_picks_up_rewrite_without_restart`（改写 index → 不重启即服务新内容）、`late_created_index_is_picked_up`（启动缺失 → 事后补建拾取）；静态面 11 单测全绿（既有 9 + 新 2）。
- 全量：cargo test 39 套件 1800 用例全绿；clippy:gate 双 crate 0 告警。
- 真机：引擎运行中改写 `dist/index.html`（注入标记注释）→ curl 立即见标记、资产引用不变（同源重建 hash 相同为预期行为——内容寻址）；随后重建 dist 恢复干净产物。

## 4. 遗留

- **待推送：433–449 共 17 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 下一个编号自 **450** 起。
