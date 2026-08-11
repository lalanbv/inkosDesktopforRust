# 18 — Phase 3 agents 域：style-analyzer（风格指纹分析）

> 日期：2026-08-12
> 范围：engine-rs（Rust 化迁移，Strangler-Fig 阶段）
> 关联：`17_Phase3_agents域_纯逻辑叶子群.md`

## 移植内容

### `agents/style_analyzer.rs`
移植 `style-analyzer.ts`（116 行）——纯文本统计（无 LLM），构建 `StyleProfile`：

- **句长统计**：均值 + 标准差（英文按词数，中文按字符数含标点）
- **段落长范围**：min/max（measure 同上）
- **词汇多样性 TTR**：英文词级（`Set/len`），中文字符级（剥标点/数字后）
- **开头模式 top5**：英文首词 lower，中文前 2 字符；过滤 <3 次
- **修辞特征**：6 中/4 英模式，≥2 次命中

## 关键技术点

- **反向引用正则的手动扫描**：TS 排比 `/[，。；]([^，。；]{2,6})[，。；]\1/g` 用反向引用 `\1`——
  Rust `regex` crate 不支持反向引用。**不引入 `fancy-regex` 重依赖**（单个模式不值得），
  改用 [`count_parallelism`] 手动扫描：定位标点 + 取 2-6 非标点串 S + 标点 + 同 S。
  与 settler_parser（lookahead 改手动扫描）同一应对思路。
- **measure 中英分流**：英文 `en_word_re().find_iter().count()`（词数），中文 `replace_all(\s+)→chars().count()`
  （含标点的字符数，对齐 TS `s.replace(/\s+/g,"").length`）。
- **TTR 中英分流**：英文词级（lower 后 HashSet），中文字符级（剥标点/数字 `zh_char_keep_re` 后 HashSet）。
- **analyzed_at 注入**：TS `new Date().toISOString()` → 调用方注入（纯内核范式，内核不依赖时钟）。
- **round1/round3**：均值/stdDev/TTR 的四舍五入（对齐 TS `Math.round(x*10)/10` 等）。

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 排比反向引用 | 手动扫描 | 不为单模式引 fancy-regex；与 lookahead 处理思路一致 |
| 12 正则 | OnceLock 编译一次 | 性能；非热路径但避免重复编译 |
| analyzed_at | 调用方注入 | 纯内核不依赖时钟；与 chapter_workspace 范式一致 |

## 验证

| 维度 | 结果 |
|------|------|
| 新增单测 | **10 项**（句长中英/段落范围/TTR 中英/topPatterns/修辞/排比扫描/source 传播）全绿 |
| 全量 lib 测试 | **455 passed**（上轮 445 + 本次 10） |
| golden 差分测试 | **22 passed** |
| clippy 双模式 | 零警告 |

## agents 域进度

纯逻辑叶子群（无 LLM，可单测）：
- ✅ detection_insights / settler_parser / settler_delta_parser / ai_tells
- ✅ **style_analyzer**（本次）
- ⬜ detector（HTTP 客户端，需 reqwest + mock 测试模式，属新测试范式）
- ⬜ writer-parser / sensitive-words（依赖 continuity.ts）

agents 域纯逻辑叶子群已移植 5 个。下一障碍是 HTTP 类（detector/radar-source）与依赖未移植模块的解析器，
需引入 HTTP mock 测试模式（wiremock）或先补 continuity.ts。
