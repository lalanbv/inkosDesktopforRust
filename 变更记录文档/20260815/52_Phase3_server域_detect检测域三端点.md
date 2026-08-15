# 52 号变更记录：Phase3 server 域——detect 检测域三端点（全章扫描 / 历史统计 / 单章检测）

- 日期：2026-08-15
- 范围：engine-rs（新增 `utils/detection_insights.rs`；扩展 `server/books_state_routes.rs`、`server/mod.rs`、`utils/mod.rs`）
- 契约源：`packages/studio/src/api/server.ts` L5892-L5911（detect/:chapter）、L6045-L6067（detect-all）、L6071-L6084（detect/stats）；`packages/core/src/agents/detection-insights.ts`（72 行）、`packages/core/src/pipeline/detection-runner.ts` 的 `loadDetectionHistory`（读取面）、`packages/core/src/models/detection.ts`（25 行）
- 验证：`cargo test --lib`（925）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（33）+ `cargo test --features export-bindings --lib`（1084）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

51 号后 books 域剩检测域三端点。`analyzeAITells` 纯函数（四维结构检测：段落等长/套话密度/公式化转折/列表式结构）已随 47 号 merged-audit 移植（`agents/ai_tells.rs`），本轮补端点装配与检测历史聚合面（`analyzeDetectionInsights` + `loadDetectionHistory`，纯函数 + 单文件读取，无 LLM 依赖）。

## 二、交付内容

### 1. `engine-rs/src/utils/detection_insights.rs`（新建，~230 行）

- `DetectionHistoryEntry`（camelCase 反序列化；字段缺省宽容：timestamp/provider 空串、score 0.0、action 空串、attempt 0）
- `DetectionStats`/`ChapterBreakdown`（camelCase 序列化）
- `analyze_detection_insights(&[entry])`：
  - 空历史 → 全零统计
  - 按章分组**保持首次出现顺序**（对齐 TS Map 迭代序，非章号序）
  - 组内 attempt 升序稳定排序：original = 首条 score、final = 末条 score、rewriteAttempts = action="rewrite" 计数
  - 均值 `Math.round(x*1000)/1000` 千分位舍入；passRate = final≤original 章占比，百分位舍入
- `load_detection_history(book_dir)`：读 `story/detection_history.json`；缺失/损坏 → 空（TS catch 语义）
- 单测 4 例：空历史/首现序聚合（2 章 5 条全字段）/passRate 舍入（2/3→0.67）/加载缺失损坏与字段缺省

### 2. 三端点（`server/books_state_routes.rs`）

| 端点 | 行为 |
|---|---|
| `POST /books/:id/detect-all` | `chapters/` 下 `.md` 且前 4 字符均 ASCII 数字 → **文件名字典序**逐一 `analyze_ai_tells(content, Zh)` → `{bookId, results: [{chapterNumber, filename, issues}]}`；读文件失败 500 |
| `GET /books/:id/detect/stats` | `load_detection_history` → `analyze_detection_insights` → 直接返回 stats；缺/坏文件 200 空统计 |
| `POST /books/:id/detect/:chapter` | padStart(4) 前缀定位 .md（无下划线要求）→ 404 `"Chapter not found"` / `{chapterNumber, issues}` |

`ai_tell_issues_json` 手动拼装（AITellIssue 无 Serialize derive）：`{severity, category, description, suggestion}`。

### 3. E2E（books52_e2e，3 例）

- detect-all：3 章字典序（0001/0002/0010，chapterNumber 取前 4 位）+ 三段等长文本触发段落等长 AI-tell（issues 非空）+ index.json/notes.md/12_bad.md 排除
- detect/:chapter：命中/缺章 404/NaN → "NaN" 前缀无匹配 404
- detect/stats：缺失空统计 → 手写 5 条历史聚合（breakdown 首现序 [2,1]、均值千分位、passRate 1.0）→ 损坏 JSON 回空统计

## 三、parity 要点

1. detect-all 的排序是**文件名字典序**（JS `sort()` 默认），非章号数值序——"0010" 排在 "0002" 后是字典序巧合正确，但 "0100" vs "0099" 类输入同样字典序（4 位前缀下两者一致）
2. detect/:chapter 与 detect-all 的语言均缺省 zh（TS `analyzeAITells(content)` 不传 language）
3. detect/:chapter 的 padStart NaN 怪癖：`String(NaN).padStart(4,"0")` = `"NaN"`；负数 `"-1"` → `"00-1"`（Rust 按同规则构造前缀，负值文件名可匹配的荒诞边界也对齐）
4. stats 的 chapterBreakdown 顺序 = 历史中章节**首次出现顺序**（TS Map 插入序），E2E 以 [2,1] 断言

## 四、偏差备案

1. detection_history.json 数值字段脏数据（如 score 为字符串）：TS 不校验继续聚合（产出 NaN 统计）；Rust serde 解析失败整文件回空统计（200 空形状，不产出 NaN 污染）
2. detect-all 读章节文件失败：TS Promise.all reject → 500 英文原生长文案；Rust → 500 io 错误短文案（状态码一致）

## 五、暂缓件

- `detectAndRewrite` 检测-改写循环与 `detectAIContent`（依赖外部 AIGC 检测 provider 配置域 `DetectionConfig`——models/project 的 detection 配置未移植，随后续 provider 配置域一并处理）
- `recordHistory` 写入面（仅被 detectAndRewrite 使用）

## 六、下一步（53 号候选）

1. books/create + create-status 端点（书籍创建向导）
2. truth 文件写端点（`PUT /books/:id/truth/:file`，L5915——白名单 + legacy shim 只读 + runtime 诊断只读三重校验已在 48 号读面移植，写面补齐）
3. import/fanfic 域端点
4. sessions/state 配置域端点（Phase 2 清单）
5. Node sidecar 下线核对：books 域累计 36+3=39 端点已可切换

## 七、影响面

- Rust 业务端点累计：36（51 号后）+ 3 = **39 个**
- 新增测试：lib +4（detection_insights 单测）、e2e +3（books52_e2e）
- 无破坏性变更；`detect/stats`（静态段）与 `detect/:chapter`（参数段）路由无冲突
