# 216 号：book references 子系统移植——引用绑定 + manage_book_reference 工具 + 写路径引用注入

- 日期：2026-09-08
- 分支：develop
- 关联：215 号（缺口 C 升正）、212/214 号（共享 golden 向量模式第三次落地，首落零漂移）
- 推送核验：origin/develop 仍停 6bc65564——**201–215 十五提交均未推送**；本批次后本地领先 16。两项默认值无新答复。

## 一、移植范围（core `references/` → engine `references.rs`）

| core 面 | Rust 对应 | 说明 |
|---|---|---|
| `book-references.ts`（257 行） | `references.rs` 持久化半区 | manifest `books/{id}/story/reference_bindings.json`（version 1 envelope + camelCase），原子替换写（temp+rename）；bind/upsert（同素材保留 createdAt）、unbind（未绑定不写盘）、list（素材可用性解析——manifest 在即 available，markdown 丢失属选段期失败，与 TS 分层一致） |
| 校验面 | 同名 fn 逐字错误文案 | `assertMaterialId`（≤240/防 `..`/路径分隔）、uses 1..=12 且单条 ≤120（trim 去重）、note ≤2000；缺 `uses` 键 → 空数组路径（TS `?? []`）；manifest envelope 非法 → `Invalid book reference manifest: {path}` |
| `loadMaterialAsset` | `load_material_asset` | TS Partial 校验面：id 一致 + title/markdownPath/manifestPath 字符串 + **markdownPath 必须等于素材 id 对应路径**（safeChildPath 双向防穿越） |
| `reference-context.ts`（166 行） | `references.rs` 选段半区 | 正文抽取（`## Extracted content` 标记）→ 标题行分节 + 前言以素材标题作节 + 空节过滤 + slug anchor 去重（`-2/-3`）→ LLM 选段（只回 source id，温度 0.1 / 2048 tokens）→ 命中节全文为 ContextSource（reason = `User-bound reference ... Reference guidance only` 定性） |
| notes 语义 | 同构 | 素材 manifest 缺失 → `book-reference-unavailable:{id}`；选段失败 → `book-reference-selection-failed`（空条目不阻断）；清单/文件面失败 → provider 层 `book-reference-context-unavailable` |

## 二、接线面

1. **工具**：`interaction/book_reference_tool.rs`——`manage_book_reference` schema（TS Params 逐字）+ list/bind/unbind 文本与 details 面（`book_reference_list/unbound/bound`）逐字；注册 book/edit 会话（TS edit 过滤器不剔除）；**不在 PRODUCTION_MUTATION_TOOL_NAMES**（绑定清单非章节/真相写入面，TS 同构）。
2. **composer**：`LlmReferenceSelector`（parse_selected_sources 白名单过滤复用）+ `build_reference_selector_messages`（TS 逐字 zh/en）；`ComposeChapterInput.reference_context_provider`——条目接在基础上下文之后（`[...base, ...reference]`），notes **前置**于预算 notes（TS 序）。
3. **写路径**：`ProductionReferenceContextProvider` 在 write-next（prepare_write_input）与 compose 端点两处生产调用点装配（对应 TS runner 单一 `referenceContextProvider` 闭包）；e2e 证实真实链注入 context.json。

## 三、共享 golden 向量（第三域）

`packages/core/src/__tests__/golden/references-vectors.json`（7 split + 3 extract + 5 slug）为唯一事实源：core `golden-references.test.ts`（15 用例）+ engine `tests/golden_references_diff.rs`（2 测试函数覆盖 15 向量）。core 侧为此导出 `splitReferenceSections`/`extractMaterialContent`/`slugifyAnchor`（原私有纯函数）。**首落即零漂移**——与前两域（beats/exports 落地即抓双端真实漂移）相比，本次移植以向量先行验证了逐字对照的完整性。

## 四、验证

- engine：lib **1302**（+14：references 10 + 工具 3 + compose 注入 1）、集成 **196**（+1 写路径引用注入 e2e）、golden refs 2、beats/export/leaf 全绿、clippy 零告警、INKOS_DUEL=1 duel **10/10 真跑**。
- core：vitest **201 文件 1917**（+15 golden references）；studio 790 复核、双 typecheck ✓。
- e2e 亮点：`e2e_write_next_injects_reference_context` 走完整 write-next HTTP 链（plan→compose→writer→落盘），断言 `chapter-0001.context.json` 含 `reference/mat1#人物关系` 条目（reason + 原文 excerpt），mock LLM 收到真实选段 prompt。

## 五、遗留

1. studio UI 无素材绑定管理面板（TS 同构缺失——`manage_book_reference` agent 工具是唯一入口，TS 亦然，非漂移）；待真实使用反馈决定是否加 UI 面。
2. `parseDraftDirectives` 死代码清理候选不变（215 号备案）。
