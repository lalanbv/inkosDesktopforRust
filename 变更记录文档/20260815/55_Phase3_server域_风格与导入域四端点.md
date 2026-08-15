# 55 号变更记录：Phase3 server 域——风格与导入域四端点（style 指纹 / LLM 文风向导 / 正典导入 / 番外正典读）

- 日期：2026-08-15
- 范围：engine-rs（新增 `server/style_routes.rs`；扩展 `server/mod.rs`）
- 契约源：`packages/studio/src/api/server.ts` L6184-L6255（style/analyze、style/import、import/canon）、L6300-L6309（fanfic 读）；core 侧 `pipeline/runner.ts` L2472-L2643（generateStyleGuide + buildDeterministicStyleGuide）、L2649-L2812（importCanon + readParentChapterSample）
- 验证：`cargo test --lib`（925）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（45）+ `cargo test --features export-bindings --lib`（1084）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

import/fanfic 域勘测后按依赖切分：`analyzeStyle` 纯函数与 `StyleProfile` 模型已随早期轮次移植（`agents/style_analyzer.rs`），`buildWritingMethodologySection` 已有——本轮补端点装配与 `generateStyleGuide`/`importCanon` 两条 LLM 链。`importChapters`（依赖 architect 基础设定生成）与 `fanfic/init`/`refresh`（canon 提取长链）、`spinoff/init`（依赖 createStatus）暂缓。

## 二、交付内容（`server/style_routes.rs`，~430 行，4 端点）

### 1. `POST /style/analyze`（L6184）

`analyzeStyle(text, sourceName ?? "unknown")`（默认 zh）→ 直接返回 StyleProfile（camelCase：8 字段 + paragraphLengthRange 嵌套）。text 缺/非串/trim 空 → 400 `"text is required"`。

### 2. `POST /books/:id/style/import`（L6199）——`generateStyleGuide` 全链

1. 统计指纹 `analyze_style` → `story/style_profile.json`（pretty 落盘）
2. 样本 **UTF-16 码元 <500** → 确定性指南（`buildDeterministicStyleGuide` 双语模板逐字：统计指纹 + 使用方式，reason 说明短样本）；≥500 → LLM 定性拆解（system 双语提示逐字、temp 0.3、默认端点路由 agent "style-guide"）——空响应/LLM 失败均回退确定性指南（reason 携带错误详情）
3. `qualitativeGuide + "\n\n" + 写作方法论` → `story/style_guide.md`，响应 `{ok, result}`
4. SSE：`style:start`/`style:complete`/`style:error`

### 3. `POST /books/:id/import/canon`（L6240）——`importCanon` 全链

1. `list_books` 双向存在校验（错误文案逐字含 Available 列表；空 → `(none)`；列表顺序为 readdir 任意序，TS 同款）
2. 读父书 8 真相（`readSafe` 失败 → `"(无)"`；story_frame 新布局优先、空回退 legacy story_bible）
3. LLM 正典生成（「网络小说架构师」系统提示逐字 + 8 段 user 提示、temp 0.3）
4. **确定性 meta 块追加**（parentBookId/parentTitle/generatedAt——防 LLM 幻觉时间戳）→ `story/parent_canon.md`
5. 父书章节样本（.md 字典序前 5、累计 <20000 UTF-16 码元）≥500 → 顺带为目标书生成风格向导（吞错）
6. SSE：`import:start/complete/error`（type: "canon"）；响应 `{ok: true}`

### 4. `GET /books/:id/fanfic`（L6300）

读 `story/fanfic_canon.md` → `{bookId, content}`；缺文件 content:null 仍 200。

### 5. E2E（style55_e2e，5 例，mock LLM 双分派）

指纹形状与 400 / 短样本确定性指南（不调 LLM + SSE 三事件）/ 长样本 LLM 指南（定性输出 + 方法论拼接）/ canon 全链（meta 块 + 顺带风格指纹 + SSE + Available 错误文案 + 400）/ fanfic null 与命中。

## 三、parity 要点

1. **500 字阈值是 UTF-16 码元计数**（TS `sample.length`），非字符数
2. **canon 的 meta 块是确定性追加**（`content + metaBlock`）——LLM 输出原样保留再补 meta，不解析不清洗
3. LLM 定性拆解的**三级回退**：短样本（不调 LLM）/ 空响应 / 调用失败——reason 文案三类逐字
4. `sourceName ?? "unknown"`：缺省回 "unknown"（落 profile.sourceName）
5. 风格向导语言：`book.language ?? genre.language`（与 51 号 resync 同源逻辑）

## 四、偏差备案

1. sourceName 非字符串真值（如数字）：TS 透传落 profile；Rust as_str 失败 → "unknown"（垃圾输入不复刻）
2. 端点 catch 的 500 文案：TS 为 `String(e)` 原生英文长文案；Rust 为链内中文/英文短消息（状态码一致）
3. LLM 调用不带 abort signal（TS `this.currentAbortSignal()`——Rust 侧中止传播面未建，全端点一致暂缓）

## 五、暂缓件

- `POST /books/:id/import/chapters`（architect 基础设定生成 + 逐章 ChapterAnalyzer 回放——architect 域大件）
- `POST /fanfic/init` + `POST /books/:id/fanfic/refresh`（initFanficBook/importFanficCanon 的 canon 提取长链）
- `POST /spinoff/init`（依赖 bookCreateStatus 内存状态机 + architect）
- `readParentChapterSample` 的 20000 上限按 UTF-16 码元累计（TS content.length）

## 六、下一步（56 号候选）

1. `GET/PUT /project` 全量配置端点（ProjectConfigSchema 全字段）
2. architect 域大件（books/create + import/chapters + fanfic/init 共同依赖：ArchitectAgent 基础设定生成）
3. Node sidecar 下线核对：累计 61+4=**65 端点**已可切换
4. `GET /project/files|artifacts` 文件浏览面

## 七、影响面

- Rust 业务端点累计：61（54 号后）+ 4 = **65 个**
- 新增测试：e2e +5（style55_e2e）；lib 无新增（复用已移植 analyze_style/StyleProfile/methodology，行为由 E2E 覆盖）
- 无破坏性变更
