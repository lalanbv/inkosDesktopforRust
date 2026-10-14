# 702 号：fork-only 面批量回收第二批（MemoryDB 只读 Mini 端口 + genre 读面，回收清单 52→40）

日期：2026-10-15。类型：fix(core)+fix(studio)+fix(scripts)。前置：701 号（f1644ecc）。

## 一、承接与实现

**只读 MemoryDB Mini 端口**（`state/memory-db-readonly.ts`，新）：node:sqlite DatabaseSync 对 legacy memory.db 的最小 SELECT 面（getAllHooks/getSummaries/listReviewMetrics/listDebts），SQL 与 fork memory-db.ts 逐字同源；**不执行 fork 构造器的 migrate()**（只读语义不建表不迁移）；类型经 core 内部类型断言（StoredHook/ReviewMetricRow 导入，StoredSummary/StoredReviewMetric/StoredQualityDebt 本地声明）。

**路由嫁接第二批（6 路由）**：promises（buildPromiseTimeline/detectPacingDebts/detectWeakHookRuns + kind 富化改用 studio union 解析器 parsePendingHooks——PendingHook.id 字段）、quality-trend（buildQualityTrend + resolvePromiseUrgency + targetChapters 注入）、quality-debts（status 过滤）、roster-candidates（extractCharacterCandidates + parseEntityRoster + resolveRosterCandidates）、roster/confirm（applyRosterConfirmation + renderEntityRoster 写面）、genres GET（genre-reader）。

**genre-reader.ts（新，692 模式）**：readGenreProfile/listAvailableGenres/getBuiltinGenresDir 自 fork rules-reader 迁出为 fork 自有资产（三级查找：项目级→内置 packages/core/genres→other.md 兜底）+ models/genre-profile.ts 恢复。

## 二、度量（三域报告实测）

**共享面 18 对比 / 0 分歧**（13→18 扩容保持全绿）：promises/quality-trend/quality-debts/roster-candidates 四面 400/400（无 memory.db 一致性——报告模式双腿对称）、genres 200/200；fork-only **40/40** 确认（52→40，又回收 6）；上游新增 9/9+2。
**门禁**：core 881 + studio 646 + cli 81 全绿、typecheck 三包 0、bindings 186。

## 三、施工实录（类型边界三连修）

1. **MemoryDBReadonly 返回类型**：裸 Record 返回会让路由端 buildPromiseTimeline/detectWeakHookRuns/buildQualityTrend 参数类型全红——**端口内部 cast 成 core 具体类型**（StoredHook/ReviewMetricRow 导入复用，StoredSummary/StoredQualityDebt 本地声明），与 fork memory-db 的内部 cast 同型。
2. **route 内 roster 变量声明缺失**（696 型嫁接事故）：token 级 handler 拼装漏 `let roster` 声明——两处补 `import("@actalk/inkos-core").RosterEntity[]`。
3. **promises kind 富化解析器换型**：fork 的 core.parsePendingHooksMarkdown（宿主已删）→ studio union 解析器 parsePendingHooks（字段名 hookId→id，698 口径）。
4. **studio lib 相对导入缺 .js**（truth-display → app-language）：node16 moduleResolution 显式扩展名要求——node --check 兜不住、typecheck 兜住。

## 四、教训

1. **只读端口是 SQLite 依赖族的正确回收形态**：不搬 fork 的 migrate/写面，只搬 SELECT——路由面恢复的最小充分实现；调用方（路由）先 access 检查文件再构造，端口自身不建文件。
2. **类型边界放在端口内部**（SQL 行→core 具体类型的 cast 收敛在 Mini 端口），路由端拿到的就是强类型——cast 散在路由里会每处都红。
3. **studio 包内相对导入必须显式 .js**（node16 moduleResolution）——从备份分支拷贝的代码块要先过一遍导入扩展名。
