# Phase 1 utils 域移植首批（book-id/language/path/length-metrics/chapter-memo-parser/cadence-policy/pov-filter）

> 日期：2026-08-11
> 范围：engine-rs Phase 1 —— 移植 packages/core/src/utils 的 7 个叶子模块
> 关联：迁移规划 v1 §4 Phase 1；记忆 `rust-migration-pivot`

---

## 1. 移植清单（全部 golden 差分验证零功能丢失）

| 模块 | TS 源 | 函数数 | 关键移植点 |
|---|---|---|---|
| `utils/book_id` | book-id.ts (31行) | 3 | derive 用单遍字符迭代+内联折叠（0GC，替代 TS 多遍正则）；is_safe 路径/控制字符/shell 元字符防护 |
| `utils/language` | language.ts (17行) | 1 | CJK/Latin 占比；合并 TS 冗余双判为单条件（语义等价） |
| `utils/path` | posix-path.ts (6行) | 1 | Windows `\`→POSIX `/` |
| `utils/length_metrics` | length-metrics.ts (133行) | 8 | UTF-16 码元计数（对齐 JS .length，emoji 计 2）；\u{feff} 对齐 JS \s；en_words ASCII 状态机；markdown 剥离 |
| `utils/chapter_memo_parser` | chapter-memo-parser.ts (160行) | 1+错误 | regex+OnceLock；thread-ref `(?-u)` 对齐 JS \b；UTF-16 minContentChars；栅栏/散文剥离 + 严格小节校验 |
| `utils/cadence_policy` | cadence-policy.ts (46行) | 1+常量 | 阈值常量 + medium/high/none 判定 |
| `utils/pov_filter` | pov-filter.ts (149行) | 3 | 前瞻分割 `(?=^###)` Rust 不支持→手写切点；动态正则；表格行分类 |
| **models/length_governance** | length-governance.ts | 3 类型 | serde camelCase 对齐 TS JSON 契约 |
| **models/input_governance** | input-governance.ts | ChapterMemo | serde camelCase |

**合计**：7 utils 模块（~22 函数）+ 2 models 类型。

## 2. golden 差分闭环（机械化零功能丢失保证）

```
packages/core/__tests__/golden-leaf-dump.test.ts   ← 调真实 TS 实现，dump 真值
        ↓ 写
engine-rs/tests/golden/utils/leaf.json             ← committed golden 真值
        ↓ include_str! 读
engine-rs/tests/golden_leaf.rs                     ← 13 差分测试逐例断言 Rust==TS
```

更新向量：`cd packages/core && npx pnpm@9 exec vitest run src/__tests__/golden-leaf-dump.test.ts`

## 3. 测试与质量

- engine-rs lib：**55 测试通过**（单元）
- engine-rs golden：**13 差分测试通过**（vs TS 真值，覆盖正常/边界/注入/Unicode/错误路径）
- clippy `--all-targets --features export-bindings -- -D warnings`：**零警告**
- ts-rs 导出：ChapterStatus/BookMeta/ChapterMemo/LengthSpec/LengthCountingMode 等类型已生成 .ts 绑定

## 4. 提交链（master）

```
2ff237ad feat(engine-rs): 移植 pov-filter
2b951167 feat(engine-rs): 移植 cadence-policy
<chapter-memo-parser commit>
<length-metrics commit>
<utils leaf batch commit>  (book-id/language/path)
```

## 5. 关键移植经验（供后续域复用）

1. **JS `.length`/`slice` 是 UTF-16 码元**——含 emoji/CJK 扩展时须用 `char::len_utf16()` 累加，`chars().count()` 会偏差
2. **JS `\s` 含 `\u{feff}`（BOM）**，Rust `is_whitespace` 不含——显式补入
3. **JS `\b`（无 /u 标志）是 ASCII 词边界**，Rust regex 默认 Unicode——用 `(?-u)` 切回
4. **TS `split(/(?=^###)/m)` 前瞻** Rust regex 不支持——手写「按 ^### 行首切点」分割
5. **serde `rename_all="camelCase"`** 对齐 TS JSON 契约（strangler 切换时端点字段名一致）
6. **regex OnceLock 编译一次**——解析器非热路径，编译一次复用，零运行时多余分配
7. **测试数据注意**：TS 字符串方法对意外子串敏感（如 "没有 POV 声明" 被 POV 正则匹配）——测试输入须避开意外命中，以隔离被测行为

## 6. 下一批候选

纯、无 node 依赖：`context-filter`(190行) / `chapter-cadence`(211行) / `chapter-splitter`(80行, CJK 数字正则) /
`writing-methodology`(164行, 静态 markdown)。`story-markdown`/`hook-*` 非 leaf（依赖 state+skills）。
