# 792 号：契约差分 legacy 通道——双栈 legacy 收敛全端点动态认证 + 四分歧收敛

日期：2026-11-01。前置：788-791 legacy 发散簇四面修复（snapshot bootstrap/durable 容忍降级/校验+去重/索引重建）均为单测级认证；本轮把认证升级为**双活体引擎全端点差分**（engine-contract-diff 新增 legacy 通道），并收敛通道首轮抓出的四处真分歧。

## 一、交付

1. **`scripts/legacy-fixture.mjs`**：向 `<root>/books/legacy-book/` 播种 1.x 迁移书形态（章节产物 + markdown 真相面，**无 state 四件、无 index.json**），并按 697 号桥镜像执行 `migrateLegacyWorks`（2.0 TS 腿 /books 列表走 works 清单，books/<id> 须注册才可见；prompt-packs 探测 fork 契约腿跳过迁移）。纯文件播种零 LLM 依赖。
2. **差分器 legacy 通道**（engine-contract-diff.mjs `runLegacyChannel`）：双腿各播一本 legacy 书 → 11 个只读端点归一化差分（books 过滤 legacy 条目/detail/chapters/workspace/truth/timeline/review-mode/series-backfill/analytics/eval/genres）。**双模式接线**：严格模式主表后 + 695 报告模式豁免记账收尾前各调一次（报告模式 ProbeEarlyExit 早退曾致首版通道未跑——接线点必须在早退之前）。
3. `/api/v1/books` 全表含 walkthrough 书且报告模式双腿播种不对称（fork 章节数刻意不同）——通道内**过滤只比 legacy-book 条目**（双端都必须含它=可见性认证）。

## 二、通道首轮抓出的四处真分歧与收敛

1. **list 缺 legacy 书**：node 腿 books/<id> 未注册 works → 补夹具 migrateLegacyWorks（697 桥）后收敛。
2. **bookDetail 行键差**：Rust ChapterMeta **无 observations/provenance 字段**（serde 忽略未知键）、audit_issues/length_warnings serde default Vec **恒序列化**——TS schema 补 `lengthWarnings` optional、`observations`/`provenance` 转 optional（存量索引照常 round-trip），重建行键与 Rust rebuild 行逐键对齐（发 auditIssues/lengthWarnings 空数组，不发 observations/provenance）。8 处 `.observations` 未守卫读补 `?? []`（edit-controller/runner×3/测试×4——其中 2 处是 perl 批量替换自伤，Edit 精修）。
3. **truth 清单序**：双侧裸 readdir 序（APFS hash 序跨根不稳定）——Rust `list_md_json_yaml` sort + TS truth handler 各子清单 sort（契约确定性）。
4. **analytics 形态分歧（预存，报告模式主表从未比过该端点）**：TS 保留 observation 两字段、缺 Rust 三字段——`computeAnalytics` 逐字移植 Rust `compute_analytics` 权威形态（topIssueCategories：issue_category_re 分类计数 stable sort 取 10 / chaptersWithMostIssues：issueCount desc 取 5 并列保序 / statusDistribution；**avgTokensPerChapter 改整除截断**——Rust u64 除法 vs TS Math.round 四舍五入=活体分歧）+ studio Analytics.tsx 面板适配（状态分布 chips/高频问题/最多问题章节/通过率卡）+ i18n 四增两删。

## 三、验证

- **legacy 通道 11/11 全绿**（双活体引擎全端点归一化一致）。
- core vitest **979/979** + studio 134 文件 + cli 23 文件全绿；tsc 三包归零；clippy 双 0；testgate engine-rs 47 目标 1956（truth sort 触 Rust）+ src-tauri 584 零漂移、RustSec 双 0；gate:ts 八步全绿（legacy 通道随 engine-contract-diff 步常驻门禁）。
- **连带收敛面**：packages/cli 三命令（analytics/review/status）适配新形态（消费面普查补盲——首轮 grep 只扫了 core/studio 漏了 cli，build 门禁兜住）；studio page-mount-smoke 夹具更新。
- 环境插曲：export-epub-smoke/门禁 test 步两轮假红=**反复 diff/smoke 遗留的散引擎进程占固定端口**（探活探到持已删临时根的旧引擎）——pkill 清场即愈，非代码回归（node 腿全程绿为证）。

## 四、教训

1. **单测认证 ≠ 双栈收敛证明**：788-791 四面修复各有单测绿，但通道首轮仍抓出四处分歧——其中 analytics 形态分歧与 legacy 无关却从未被任何门禁比过（报告模式端点覆盖盲区）。双活体差分是唯一的全端点裁判。
2. **接入既有差分器必须读它的早退流**：ProbeEarlyExit 报告模式早退让首版通道静默未跑（exit 0 假绿）——扩展多模式脚本先画控制流图。
3. **schema 双侧演化差是分歧之源**：TS ChapterMetaSchema 的 observations/provenance 是 fork 时代字段，Rust 模型早已删除——「逐字对齐 TS schema」注释与现实的偏差要用差分器实证校准。
4. **形态变更的消费面普查要跨全 workspace**（core/studio/**cli**/scripts）：cli 三命令的 observations/analytics 消费漏查致 build 门禁假红两轮；模型字段 optional 化是跨包 API 变更。
5. **多轮 diff/smoke 的固定端口引擎会互相幽灵化**（旧进程持已删根应答新探活）——跑前 pkill 清场；探活通过≠新引擎就绪。
