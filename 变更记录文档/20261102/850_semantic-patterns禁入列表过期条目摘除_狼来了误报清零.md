# 850 号：semantic-patterns 禁入列表过期条目摘除——狼来了误报清零

日期：2026-11-02。前置：849 号。确认轮顺带暴露的卫生项升级为小实质轮（其余巡检项廉价实跑全维持：上游 0、serde_json 1.0.151、advisory-db 当日；bench 不重复——846 后零生产代码变更且 848 收口时 gate:ts 八步刚全绿）。

## 一、发现

849 号复核 `audit-semantic-patterns.mjs` 输出时注意到首行 finding 恒为：

```
packages/core/src/models/genre-profile.ts:1  forbidden 1.x source remains
```

该文件在 `FORBIDDEN_SOURCE_PATHS`（六条 1.x 禁入路径）里，但实际**存在且合法**：上游 2.0 合并带来的是 zod 流派档案 schema（`GenreProfileSchema` / `parseGenreProfile`），与 1.x 被禁的 genre 命令语义彻底换代——纯属路径撞名。列表条目过期 → 工具**每次运行必报一条狼来了**，训练使用者忽略其输出（信息型人审清单的价值全在可信度上）。其余五条禁入路径核实确实不存在（守卫对真残留仍有效）。

## 二、实施

摘除 `genre-profile.ts` 条目，原位留注（路径撞名经过、摘除理由、849 号实测）。验证：`forbidden remains` 行 0 条、candidates 42→41（其余 41 条为语义启发式人审候选，工具本义不变）、语法 ✓。

## 三、教训

**禁入/禁改类清单要周期性对着现实重核**——「文件仍存在」的告警可能是列表过期而非残留未清；路径级守卫在路径被合法复用时退化为常驻误报，与 846 号恒绿同属「告警长期同形 → 被无视」家族。摘除时原位留注（为什么摘、何时摘），防未来误加回。

## 四、出口

维持：上游数值分协议扩展 / serde_json >1.0.154 解除 / @streamdown/math katex 0.18 / pi1.0 key / 新真实需求。**用户侧待办：Fork 推送 3 个提交（22ec3047/848、d87163b4/849、本轮）。**
