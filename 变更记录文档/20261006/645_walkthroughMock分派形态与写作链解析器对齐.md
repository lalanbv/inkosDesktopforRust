# 645 号：walkthrough-mock 分派形态与写作链解析器对齐——活体走查零 fallback 化（644 备案清偿）

日期：2026-10-06。类型：chore(scripts 走查基建)。前置：644 号备案「mock 与写作链解析器形态差距（planner fallback×3/审稿解析失败/62 字篇幅不足=走查环境常态 fixture 不红）另议」。本号单文件改动（scripts/walkthrough-mock.mjs），红绿以活体 env 日志+API 章节状态闭环。

## 一、三个形态差距的根因（对照生产解析器）

1. **planner memo parse failed×3 fallback**：memo 解析器（core chapter-memo-parser）要求 9 个必备节各 ≥20 字符（Phase hotfix 7 非空阈值）；mock 的 PLANNER 有六节内容只有 12~19 字符——「当前任务」恰 19 字差 1。恒 3 次重试后走 fallback memo。
2. **审稿输出解析失败**：continuity 审稿的输出契约是 JSON（`parseAuditResult` 四策略：平衡 JSON/纯 JSON/\`\`\`json 块/正则抽 "passed"）；mock 的审稿分支输出 `"PASS\n95"` 纯文本，四策略全不命中 → parseFailed → 审改循环跳过 + 章节落 `audit-failed`（auditIssues 记 critical「审稿输出格式异常」）。
3. **62 字篇幅不足**：篇幅带按 book.chapterWordCount=3000 推导（hardMin 2182/hardMax 3818，zh_chars 去空白计数）；mock 正文 62 字恒触发警告。645 实测还牵出第四个隐藏面：**反 AI 审查栈**（post-write-validator + ai-tells）——首版扩写正文命中「不是…而是…」critical、「——」破折号 error、段落长度变异系数 0.117<0.15 warning，章节仍 audit-failed。

## 二、mock 形态修复（对齐生产解析器预期，解析器零改动）

- **PLANNER**：六个短节各扩写到 25~45 字符（保持 markdown 列表/句子语义形态），本地自检脚本按解析器同口径逐节断言 ≥20。
- **审稿分支**：改为 JSON 契约形态 `{"passed":true,"overall_score":88,"issues":[],"summary":"…"}`。
- **WRITER 正文**：扩写至 2405 去空白字符（带内），且整表规避审查规则全表——无「不是…而是…」/无破折号/转折标记词（仿佛·忽然·竟然·猛地·不禁·宛如）全文 0 次/公式化转折词 <3/元叙事与章节号指称 0/段长长短交错 cv=0.463/连续同前缀句 1。自检脚本按 post-write-validator 与 ai-tells 同口径正则逐条断言。

## 三、红绿（活体）

- **红**（修复前 env 实录，`/tmp/walk-645-red.log`）：planner fallback×6（两轮链各 3）+ 审稿输出解析失败 + 第 2 章 62 字；API 层章节 `status:"audit-failed", wordCount:62`，auditIssues 含 critical「审稿输出格式异常」。
- **绿**（修复后 green3）：全链缺陷计数 **0**（fixture 一轮+API 一轮 write-next）；章节 `status:"ready-for-review", wordCount:2405`，auditIssues 仅余两条非阻塞 warning（标题自动调整通知/章节类型节奏提示——审查器正常输出，非 mock 形态缺陷）。
- **门禁**：gate:ts 八步全绿（node-fallback-smoke 双腿即新 mock 的活体回归，61 端点 0 分歧）。Rust 零触碰裁剪备案（535 先例；本号纯 scripts）。

## 四、备案

- 走查书章节状态从恒 `audit-failed` 变 `ready-for-review`——书详情页不再满屏红标，走查体验与真实 LLM 链路形态对齐。
- 审稿 JSON 的 `overall_score:88` 是 mock 恒定值；审改循环/降分重写分支（<65 分形态）未被本号覆盖，若需走查审改循环可给 mock 加可注入降分形态（WALKTHROUGH_MOCK 环境变量惯例已有先例）。
- mock 正文为固定文本，跨章重复检测（跨章词汇疲劳）在多章连续写作时可能累积 warning——非阻塞，观察。

## 五、教训

1. **mock 的输出形态是生产解析器的隐含契约**：mock 按「人看起来像」的形态写（PASS\n95、DIMENSION 评分表），解析器按「生产 LLM 被提示词约束的形态」解析（JSON）——形态对齐必须以解析器源码为准逐条核对，不能凭直觉。
2. **阈值型校验器要拿到阈值再写 fixture**：memo 20 字符/篇幅 2182-3818/段落 cv 0.15/转折词 ≤1 次/3000 字——差 1 个字符就是 3 次重试+fallback；本地按同口径自检脚本先行，比活体试错快一个量级（本号绿轮前两次因字数贴边被自检拦下，未浪费活体轮）。
3. **篇幅不是唯一审查面**：扩写正文后暴露反 AI 审查栈（句式禁令/破折号/标记词密度/段落等长）——mock 文本要过的是**全部**下游校验器，扩写前先把校验器全表 grep 出来一次对齐，否则红绿循环在审查面上继续裂变。
