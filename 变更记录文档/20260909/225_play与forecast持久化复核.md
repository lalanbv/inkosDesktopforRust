# 225 号：play_tools 状态机 + forecast 持久化面复核——互动存档原子写升级

- 日期：2026-09-09
- 分支：develop
- 关联：80 号（play 工具面）、91 号（play runner）、109/115 号（forecast 三件）、221 号（原子写先例——本批扩展覆盖面）、116 号（AtomicFileSet/`write_file_atomic` helper 复用）
- 推送核验：origin/develop 仍停 6bc65564——**201–224 共 24 提交待推送**；本批次后本地领先 25。两项默认值无新答复。

## 一、审计范围与结论

| 审计点 | 结论 |
|---|---|
| play 会话绑定/隔离 | ✓ worldId === sessionId；`safePlayId` 逐字（80 码元截断 + 拒绝 `.`/`..`/分隔符/NUL）；注册条件探测对非法 id 视为不存在 |
| play_step 失败面 | ✓ 优雅降级（不把原始 LLM 错误交给外层 agent）；runner 层瞬时错误重试正则在位（play_runner 113 行） |
| **play 持久化原子性** | **缺口（已修）**：world.json（create/update 双写点）、settings、state/current.json、manifest、编辑/导出文件、events/transcript 快照重写共 11 处 `tokio::fs::write` 直接覆盖——play_step 中途崩溃可损坏互动存档（用户进度丢失） |
| **forecast 持久化原子性** | **缺口（已修）**：forecast.json（创建 + stale 标记更新两处覆盖写）、comparison.md、selected-branch-plan.md 共 4 处同形态 |

## 二、修复

统一复用既有 `utils::atomic_file_set::write_file_atomic`（116 号 helper，199/221 号同族模式）替换全部 15 处覆盖写：play.rs 11 处（manifest/settings/world×2/current.json/world 文件编辑/导出/events+transcript+current 快照重写）+ forecast/store.rs 4 处（forecast.json×2/comparison.md/selected-branch-plan.md）。temp+rename 语义：崩溃保留旧文件，互动存档与推演数据不再可能半份落盘。

## 三、验证

- engine：lib **1310**、集成 **196**、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**——play/forecast 全链既有测试在原子写替换后全绿（行为零变化，仅持久化健壮性提升）。
- TS 零改动。

## 四、遗留

TS PlayStore/forecast 写入同为非原子——回退端随绞杀者进程自然淘汰，不单独修。
