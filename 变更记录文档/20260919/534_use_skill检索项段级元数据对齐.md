# 534 号：use_skill 检索项段级元数据对齐——heading/charStart/charEnd 随命中透出

日期：2026-09-19
提交：本文件同批路径限定提交
类型：fix(engine)——检索留痕精度 + 激活注入位置精度

## 走查（532 号备案双候选）

1. **en-prompt-sections 残余面复核 → 零残余排除**：双端均只剩
   `build_english_genre_intro` 被 writer-prompts 引用（TS post-write-validator.ts:465
   仅为注释提及 IRON LAW 非代码引用），529 号清偿已彻底；
2. **use_skill 留痕面核对 → 发现精度缺陷（本轮修复）**。

## 发现：检索分支段级元数据丢失

use_skill 的 query 分支（BM25 检索 skill references 相关分段）双端对照：

- TS `retrieveSkillResources`：分文档入索引时 `metadata: { path, heading,
  charStart, charEnd }`（splitMarkdownForSearch 的分段级精确位置），命中映射
  `path: hit.metadata?.path ?? ""` 等逐字段透出；
- Rust `retrieve_skill_resources`：SearchDocument.metadata **恒 None**，命中映射
  硬编码 `"heading": Value::Null, "charStart": 0, "charEnd": body 全长`——段级
  元数据三件全丢，仅 path 从 source 字符串剥出。

**连锁影响**：该映射同时是 532 号激活写回的资源来源（query 分支 retrieved →
ActivatedSkillResource）→ 注入段 "#### Reference: path:start-end · heading" 的
heading 恒缺、位置恒 0..全长——同轮 sub_agent 的 skill 参考注入位置精度受损；
details.retrievedResources（前端 RunLog details 通用展示）同样失真。

## 修复（Rust 单侧——metadata 管道 TS 已有、Rust 既有字段未用）

- `SearchDocument.metadata` 入索引时携带
  `{ path, heading, charStart, charEnd }`（对齐 TS；SearchHit.metadata 既有透传
  零改动）；
- 命中映射改为 metadata 优先（path 空 → 回退 source 剥取；charEnd 0 → 回退
  body 全长——TS `?? 0`/`?? body.length` 兜底语义同构）；
- 测试：query_retrieves_relevant_references 补三断言（path 均 .md 文件、heading
  命中分段标题"开局布局"、char 区间为真分段位置非 0..400 全长）。

## 门禁插曲

- cwd 残留伪失败：上一步 cargo 命令把持久 cwd 留在 engine-rs，`node
  scripts/engine-contract-diff.mjs` 相对路径 MODULE_NOT_FOUND（记忆"相对路径前
  先核 cwd"第三次应验）——回仓库根重跑即绿；
- gate:ts test 步骤 166s（基线 75s 的两倍）但全绿——无并行负载下偶发变慢，
  记录观察不立案。

## 门禁（8 项全绿）

| 门禁 | 结果 |
| --- | --- |
| cargo test engine-rs | 1806 passed / 0 failed（落盘求和） |
| cargo test src-tauri | 578 passed / 0 failed（落盘求和） |
| clippy:gate（--all-targets 双 crate） | 0 告警 |
| gate:ts:fast | typecheck 18.6s + test 166s + audit:npm 全绿 |
| 契约差分器（活体） | 41 对照 0 分歧 |
| node-fallback-smoke | 双引擎一致性全部通过 |
| export-epub-smoke | EPUB 结构校验通过 |
| INKOS_DUEL=1 strangler_duel | 10/10 真跑 |
| bench:gate | 负载高峰（70%/核）后台轮询低谷窗口通过，9 基准零回退 |

## 教训

- **字段管道在、数据没进去**：SearchHit.metadata 透传链路（241 号建）一应俱全，
  写入端 None 恒空导致下游全部失真——对照审计要查"链路两端"，中段字段存在性
  不代表端到端生效；
- 兜底语义要对齐而非自创：TS `metadata?.path ?? ""`/`?? body.length` 的回退面
  （缺失时退化到 source 剥取/全长）在 Rust 侧同构保留，修复不改变缺失行为只
  补齐存在行为。

## 下一号

自 535 起（开号前双查当日目录最大号；533 号已被并行会话 radar 路由轮占用——723a67a4，撞号后改号实录见本条）。备案候选：聊天轮 use_skill 激活的
RunLog/tool_executions 前端展示核对（details 通用展示面）；Rust pub API 死代码
使用面审计（"公开 API 一并移植"类残留系统扫一遍）。
