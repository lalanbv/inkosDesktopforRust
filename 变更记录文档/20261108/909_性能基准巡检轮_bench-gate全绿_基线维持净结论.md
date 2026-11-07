# 909 号 · 性能基准巡检轮：bench:gate 全绿（基线自 11-01 维持）——巡检净结论

> goal「最佳高性能 0GC」对齐面：bench 电池自 11-01（848 时代修后）以
> 来 12+ 轮未巡检。本轮复跑 bench:gate（338s）——**门禁通过**（内置
> 阈值判定），数值对照在噪声带内，基线维持；严禁动 baseline（洗基线
> 禁令记忆）。零代码改动轮。
> 验证：bench:gate ✓ 全绿；gate:ts 八步全绿（905-908 同日多轮连绿维
> 持）；零 Rust/TS 生产代码改动。

## 一、bench:gate 复跑结论

- 基线快照 `bench-baseline.json` collectedAt=**2026-11-01**（覆盖
  backup_export/chapter_index/export_artifact/paragraph_scan/read_chain
  七面/sensitive_words 双面/sse_broadcast 等 14+ 基准族）。
- 本轮对照（门禁内置阈值判定）：数值波动均在噪声带——改善面
  （projections_summaries −20.6%、tool_registry lookup_last_hit −12.3%、
  projections_hooks −6.4%）与上浮面（resolve_timing +4.5%、
  title_dedup +3.9%）混合，无越阈项；**✓ bench 门禁通过**。

## 二、净结论

- 性能基线自 11-01 维持一周，898-908 十一轮（deadline/SSRF/threads/
  依赖刷新/差分器豁免等全部改动面）无性能回归——bench 电池作为常驻
  门禁的定期巡检面本轮恢复（后续巡检周期含 bench 面）。
- 高负载误报纪律（bench-gate-load-noise）：本机低负载串行语境跑得
  绿即真绿，基线不动。

## 三、验证

- bench:gate ✓（338s）；gate:ts 八步全绿（905/906/908 三轮当日连绿
  维持，908 轮 diff 步 36.5s）；零生产代码改动。

**下一号自 910 起；出口清空回归常备；用户侧待办=Fork 推送 43 个提交。**
