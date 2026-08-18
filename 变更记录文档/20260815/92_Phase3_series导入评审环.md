# 92 号变更记录：series 导入评审环（importMode=series 完整语义）

## 一、背景

91 号交付 resumeFrom 续放与 importMode 直通时备案：series 模式在 Rust 侧只直通了 `generate_foundation_from_import(Series)` 的提示词分支，未接 TS `generateAndReviewFoundation` 评审环（生成 → foundation-reviewer 评审 → 不过则带反馈重生成）。本轮（92 号）补齐该环，闭合 91 号偏差备案 1，importMode=series 语义完整。

## 二、交付

### 1. `book_create_routes.rs`：`generate_and_review_foundation_import`（新建）

对齐 TS `generateAndReviewFoundation`（mode "series"）：
- **环结构**：`generate_foundation_from_import(Series)` → `review_foundation`（`FoundationReviewMode::Series` 衍生五维，无 sourceCanon/styleGuide）→ passed 返回；不过 → `build_foundation_review_feedback`（zh `## 总评\n…\n\n## 分项问题\n- {name}（{score}分）：{feedback}`）作为 reviewFeedback 重生成 → 循环 max 2 轮（TS `foundationReviewRetries ?? 2`）→ **终审兜底接受**（不再重生成，结果丢弃仅日志语义）。
- **错误面**：环内评审失败硬传播（对齐 TS review 抛错即导入失败；与确认面 multi 环一致）；终审容错。
- 复用件：`FoundationReviewerChat`（RoutedAgent "foundation-reviewer"）、`build_foundation_review_feedback`（70 号）、`ReviewParams`/`FoundationReviewMode::Series`（70 号已备）——零新依赖，纯装配。

### 2. 接线：`import_chapters_chain_with_resume` Step 1

series 分支（`matches!(import_mode, ImportMode::Series)`）改走评审环（reviewer RoutedAgent 就地装配）；continuation 分支维持直生成。REST 端点（薄壳 start_from=1 + Continuation）行为不变。

### 3. E2E（`mod sub92_e2e`，1 测试）

`chat_series_import_reviews_and_regenerates_foundation`——聊天面 `importMode:"series"` 导入：
- mock：architect 按反馈轮分流（消息含 `## 总评` → B 稿，否则 A 稿，角色名林动/林震区分）；reviewer 计数（首审 REJECTED 总分 66 → 之后 PASSED 总分 87）；analyzer 固定输出。
- 断言：卡片 completed + details.importMode == "series"；**评审恰两次**（首拒 + 复审过）；落盘地基来自反馈重生成轮（volume_map 含 B 稿标记"系列新程"、roles 含"林震"）；章节回放照常落盘。

## 三、parity 要点

- 环轮数（2）、反馈格式（双语）、终审兜底接受、series 五维评审（`derivative_dimensions` 的 series 分支）均对齐 TS。
- reviewFeedback 经 architect 提示词注入（`generate_foundation_from_import` 第 6 参，62 号已有参数面）。

## 四、偏差备案

1. **日志面**：TS 每轮打 `Foundation review: {score}/100 PASSED|REJECTED` 与分项行；Rust 侧环不打日志（与确认面 multi 环同 reductions——进度面由 stages 承担）。
2. **config.foundationReviewRetries**：TS 可经配置覆盖轮数；Rust 固定 2（BooksRuntime 无该配置位；与确认面 multi 同款硬编码）。

## 五、暂缓件（滚动）

- PDF 文本抽取（83 号）——独立依赖选型轮。
- 散件收尾：单章写作中途截断、/agent model 校验、resumeFrom REST 面、fetchWithProxy、attachments 归一化、模型四层解析。
- sidecar 契约差分扫描（迁移收尾的系统对齐基线）。

## 六、验证基线

| 项 | 结果 |
|---|---|
| `cargo test --lib` | 1126 过（无新增单测——环逻辑由 E2E 全链覆盖） |
| `cargo test --test golden_leaf` | 76 过 |
| `cargo test --test e2e_write_next_contract` | **156** 过（+1：sub92） |
| `cargo test --features export-bindings --lib` | 1285 过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| TS `packages/core` vitest | 185 文件 / 1798 测试全过 |

## 七、影响面与下一步（93 号候选）

导入域的聊天面语义至此完整（continuation 全量重建 / resumeFrom 续放 / series 评审环）。下一轮候选：

1. **首选：散件收尾批**（resumeFrom REST 面——import 端点补 resumeFrom/importMode 参数；/agent model 校验；attachments 归一化；模型四层解析；fetchWithProxy——多为小参数面，一轮可清多件）。
2. 其次：sidecar 契约差分扫描（以 TS 集成测试清单为基线，对已移植端点做系统对齐——strangler 切换前的收尾基线）。
3. PDF 文本抽取选型轮（unpdf/pdf.js 的 Rust 对应物评估：pdf-extract vs lopdf 自实现文本层抽取）。
