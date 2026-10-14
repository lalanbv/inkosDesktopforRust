# 698 号：chaptersWritten 对齐与共享面清零（三域报告达到迁移完成态验收线）

日期：2026-10-15。类型：fix(scripts)。前置：697 号（7a2be6dc）。

## 一、承接（697 号剩余 2 分歧：/books/0/chaptersWritten + /doctor）

## 二、根因链（四层，逐层活体定位）

1. **fixture 播种缺进度链**：`resolveDurableStoryProgress` 要求 `chapters/index.json` 与章节文件一致（`^\d+_` 扫描不认 683 号连字符探针形态，index 缺失即 ENOENT→0）——TS chaptersWritten=0。
2. **首版修复反噬**：补 index.json + canonical 孪生后 Rust chaptersWritten=**2**——Rust 把两种形态都计章（连字符+下划线各一文件），TS 计 1，从「0 vs 1」变「1 vs 2」。
3. **执行深度不对称**：Rust 腿跑完整链（resync+write-next 产出第 2 章），TS 腿停在播种态——即使单形态播种，Rust 跑链=2、TS=1。
4. **正解=报告模式双腿对称**：双腿都跑 resync（进度注册：Rust 经 memory.db 投影、TS 经迁移后的 works 面——697 号迁移使 works/<id>/work.json 在位，resync 得以通过）+ 都跳过 write-next（跳过执行深度差）。**双腿对称停在 1 章 → 1=1**。
5. **收尾**：/books 残余 `incompleteWorkIds`（TS 超集：2.0 加法字段）与 /doctor（Rust 超集：bookIssues/retrieval 诊断扩展）= **声明性超集形状**——报告比较升级为「prune 规范化 + 交集键对比 + 超集键声明日志」，超集键不拦截但有记录。

## 三、结果（contract-diff 实测）

**共享面 8 对比 / 0 分歧——迁移完成态验收线达成**：
- /books ✓（声明超集键：node=[incompleteWorkIds]）
- /doctor ✓（声明超集键：rust=[bookIssues,retrieval]）
- sessions/skills/asset-library×3/notify 全绿（skills 为 696 号 prune 口径修正的连带转绿——键序假分歧）
- fork-only 52/52、上游新增 9/9+2 深链声明、exit 0

## 四、施工实录（四断点，ESM/锚点/嵌套全凑齐）

1. fixture 顶层直线脚本（ESM 禁 return）——早退用 `process.exit(0)`（同 die() 惯例）；
2. 692 号同型：多块嫁接的括号嵌套错位（SyntaxError: Unexpected end of input），node --check 兜住；
3. WALKTHROUGH_REPORT_MODE（双腿）与 WALKTHROUGH_SEED_REPORT（仅 node 腿）两个 env 职责分离；
4. 报告模式双腿跑 resync 后 exit 0，链断言（promises/run-log）随生产链一并跳过（断言验的是链，链被跳过则断言必假红）。

## 五、教训

1. **执行深度是对称性比对的第一变量**：双腿在同一断点停（播种态）才有可比性——「一边跑了全链一边没跑」的 chaptersWritten 差异是执行痕迹不是契约分歧；度量前先让双腿处于同一执行深度。
2. **Rust 的章计数把连字符形态也算章**（TS 进度链只认 `^\d+_`）：683 号探针形态对 Rust 是真章——跨引擎的「什么算一章」本身是契约面，探测夹具不得单方面引入引擎计数不对称的形态。
3. 超集键声明是兼容性演化的常态形态：TS 加法字段与 Rust 诊断扩展在共享面上以「交集对比+声明日志」处理，既不互相追逐形状也不吞掉真分歧。
