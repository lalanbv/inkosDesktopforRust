# 81 号变更记录：play_edit 聊天工具（世界卡持久编辑）

- 日期：2026-08-14
- 阶段：Phase3 strangler 迁移（engine-rs）
- 契约源：`packages/core/src/agent/agent-tools.ts` L2163-2295（`PlayEditParams` / `createPlayEditTool`）、L2525-2590（`mergeContract` / `upsertPlayEditEntity` / `resolvePlayEditEntityId` / `playEditEntityId`）、`packages/core/src/play/play-store.ts` L88-110（`updateWorld`）、`packages/core/src/__tests__/agent-tools.test.ts` L1160-1300（两个契约测试）

## 一、背景

80 号接通了 play 会话的玩主循环（play_step / play_revise），但系统提示词中引用的 play_edit（改世界规则/视觉契约/persona/实体卡）工具本体缺失——用户要求改规则时模型得到 Unknown tool（80 号偏差备案第 5 条）。本轮补上最后一块：play_edit 不推进时间、不生成场景，只持久编辑世界卡与图谱实体，并把结果写回 currentState（graphEditedAt）。至此 play 会话三个聊天工具全数在位，与 TS `agent-session.ts` 注册面（world 存在时 play_edit + play_revise + play_step + material 两件）中 Rust 已交付的子集完全对齐。

## 二、交付

1. **`engine-rs/src/play.rs`**：`update_world`（TS `PlayStore.updateWorld` 逐字）——load → patch 键合并（worldContract/visualContract/premise/mode）→ updatedAt 重写 → pretty + 尾换行落盘；世界缺失 → `Err("Play world not found: {id}")`
2. **`engine-rs/src/interaction/play_tools.rs`**（+5 单测，共 10）：
   - `merge_contract`：整文替换优先 → from/to 全量替换（`split/join` = replace all；缺失/空跳过）→ 末尾追加（已含跳过；空文直取 add；否则 `trim\n- add`）
   - `play_edit_entity_id`：label 小写 → 非 `[a-z0-9 汉字]` 连续段折叠单 `_` → 首尾剥 `_` → 48 码元截断 → `{type}_{ascii}`；纯符号 → `{type}_{ms 36 进制}` 回退
   - `resolve_play_edit_entity_id`：显式 id 优先；label 精确匹配 snapshot.entities 的 label 或 id（首个命中）
   - `upsert_play_edit_entity`：七字段规范实体（createdEventId 保序 / updatedEventId="manual-edit"）；既无可解析 id 也无非空 label → 跳过；summary/status 保留显式空串（JS `??` 语义：显式清空 vs 未提供保留旧值），label 走真值语义
   - `tool_play_edit`：无世界 → 双语文案；契约/视觉/premise 三路 patch（变化才入 patch，空 patch 不写 world.json）；persona → actor_player 规范 upsert（label 保旧或 玩家/Player，status 已更新/Updated）；entityUpdates 循环；db.flush；currentState 合并写回（worldContract/visualContract/premise + graphEditedAt，保留 turn/scene 等既有键）；details `play_world_updated`（world 全量 + 三布尔 + updatedEntities + graph）
   - `play_tool_schemas` 追加 play_edit 条目（PlayEditParams 逐字：10 个可选字段 + replacements 数组 {from,to} + entityUpdates 11 枚举 type）
   - `execute_play_tool` 分发加 `"play_edit"`
3. **E2E `play81_e2e`**（1 测试）：play 会话聊天"把规则里的 X 改成 Y" → mock 首轮 play_edit 工具调用 → 执行卡 completed + result=note；world.json 替换生效 + updatedAt 重写；currentState 合并写回（turn/lastEventId 保留 + graphEditedAt + 新契约）；persona → actor_player 实体 manual-edit 事件位；断言无 events.jsonl（不推进回合）；注册面断言 tools 含 play_edit/play_step

## 三、parity 要点

- **不推进回合**：play_edit 全程不产生事件（E2E 断言 events.jsonl 不存在）、不渲染场景——与 play_step 的"改设定 ≠ 一回合剧情"边界互斥
- **替换 vs 追加**：`worldContractReplacements` 用 `replace`（全量替换），`Append` 仅窄新增且已含去重——对齐 TS 测试"replaces Play contract wording instead of appending conflicting rules"
- **patch 最小化**：三个契约字段变化才写 world.json（updatedAt 只在真写入时重写）；premise 仅显式提供且不同才 patch（`Boolean(patch.premise)` 报告）
- **currentState 合并语义**：非 object 或缺失 → 整体替换为 `{}`（TS `.catch(() => ({}))` + spread 守卫）；既有键（turn/scene/lastEventId）全保留
- **实体 upsert 全量替换**：七字段规范记录覆盖旧实体（TS zod parse 重建等价），createdEventId 保留旧值、updatedEventId 恒 "manual-edit"

## 四、偏差备案

1. **updateWorld 的 zod 规范化**：TS `PlayWorldSchema.parse` 重建（剥离未知键 + 默认值）；Rust 原样保留 world.json 既有键——Rust 写入面即 canonical 形态，实际等价
2. **实体 label 匹配顺序**：TS `snapshot.entities.find` 依赖 `localeCompare` 排序；Rust 按 id 字节序排序后首个命中——同 label 多实体时命中可能不同（实际场景罕见，label 本应唯一）
3. **playEditEntityId 的 toLowerCase**：JS Unicode 全量小写 vs Rust `str::to_lowercase`——对 CJK 恒等，极端 Unicode 边缘字符理论可差（无实际影响）
4. **tools 无 required 顶层字段**：PlayEditParams 全字段 optional（TypeBox 默认不产 required 键），Rust schema 同样不写 required——与 TS 线上形态一致

## 五、暂缓件

- play 会话 material / material_retrieval / propose_action 聊天工具（无世界时的工具集）
- play en 提示词补齐（interpreter/mutator/renderer/context brief 标签——reconciler 与 replayContext 已双语）
- 聊天面空文本 + 工具成功 → `response: ""` 的 TS 分支形态（66 号范围）
- 散件：单章写作中途截断、/agent model 校验、resumeFrom、fetchWithProxy、attachments 归一化、模型四层解析

## 六、验证基线（2026-08-14）

- `cargo test --lib`：**1084**（+5：play_edit 单测）
- `cargo test --test golden_leaf`：76
- `cargo test --test e2e_write_next_contract`：**136**（+1：play81_e2e）
- `cargo test --features export-bindings --lib`：**1243**（+5）
- `cargo clippy --lib --tests --bins`：零警告
- TS：`packages/core` vitest 185 文件 / **1798** 测试全过

## 七、影响面与下一步

- 影响面：`play.rs` 新增 `update_world`（纯增量，无调用方变更）；`play_tools.rs` 工具面从 2 扩到 3（schema 顺序 play_edit/play_step/play_revise——80 号 E2E 断言已随本轮回调适配）；80 号偏差备案第 5 条（play_edit 提示词段落无本体）就此关闭
- 下一步（82 号候选）：**首选 play en 提示词补齐**（interpreter/mutator/renderer 三代理 + context brief 标签双语，73 号备案遗留）；其次 material/retrieve_material 聊天工具面；或散件收尾（单章截断/model 校验/resumeFrom/attachments/模型四层解析）
