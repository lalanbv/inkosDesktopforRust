# 1590 semantic 审计狼来了清偿 + 分级改造轮（2026-11-20）

## 定案：41 candidates 常态红（850 号时点已红）=846/847 纪律失守面

逐功能扫独立验证面时浮出：`audit:semantic-patterns` exit 1 恒红（**850 号时点即 candidates 42→41 红着收官**，恒红被搁置）且不进 gate:ts 常规链=红了无人知。精确分型：**31 条令牌命中 + 9 条启发式（isLikelySemanticDecision 的 `.includes(` 业务判定形态）+ 1 条 harness entry**。

## 逐条定性（854 纪律：白名单按语义定性+精确钉定）

- **31 条令牌命中全为 fork 刻意保留的产品功能面**（非上游残留）：章节状态契约 `audit-failed`（187 号 BookTimeline/BookDetail 消费）×8、整书评估 `qualityScore`（book-eval 功能核心产物）×6、`lengthWarnings` 写链契约 ×7、修订面 `repairScope`/`reviewObservations` ×5、模型校验 `.max(20)` ×4、`AuditIssue` 对齐注释 ×1——全部有历轮变更记录背书。
- **9 条启发式误报**：业务 includes 判定（interactive-world/short-fiction/quote 校验等）——非上游残留，启发式过宽的常驻候选。
- **1 条 harness entry**：`importFanficCanon` studio 直调——定性=fork 数据导入 API（agent-tools.ts:1298 已接工具权威+studio 导入按钮活体消费），非 agent 动作链绕道。

## 脚本分级改造（三处）

1. **FORBIDDEN_ENTRY_CALLS 摘 importFanficCanon**（850 先例：清单对现实重核；其余条目零调用=守卫防回归仍有效）；
2. **ALLOWED_TOKEN_SITES 白名单**：14 文件 ×31 令牌精确钉定（file 精确匹配+令牌子串——文件级漂移仍会红）；
3. **findings 分级**：启发式分离为 candidates 报告面（不计 exit，输出带 [candidate] 标记+--json 分段），exit 只看禁令 findings。

## 红路活体闭环

探针行（非白名单文件含 audit-failed）→ **findings:1 红**（精确 file:line:1656）→ 删探针 → findings:0 绿+whitelisted 31。脚本红面证明能红。

## 验证

- audit:semantic-patterns **findings: 0 / candidates 9（report-only）/ whitelisted 31 / exit 0**
- gate:ts 十腿全绿；testgate 双 crate 2731 passed；clippy/fmt 双 0；U+FFFD 净。

## 后续

- 下号自 1591 起；用户侧待办不变（Fork 推送 955-1590 + 打包三脚本重烤 + e2e_startup 插件面复核）。
