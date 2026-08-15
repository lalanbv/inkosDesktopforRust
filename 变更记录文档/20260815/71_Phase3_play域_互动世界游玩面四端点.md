# 71 号：play 域 —— 互动世界游玩面四端点

**日期**：2026-08-15
**阶段**：Phase3 strangler 迁移（62 号清单清缺口：play 域 4 条；缺口 12 → 8 条）
**契约源**：`packages/studio/src/api/server.ts` L4551-L4700（四端点）+ `packages/core/src/play/play-store.ts`（读取面与路径安全）+ `play/play-db.ts`（sqlite 快照 SELECT）+ `play/play-file-db.ts`（文件后端快照）+ `play/play-db-factory.ts`（后端选择）+ `play/play-image.ts`（插图 sidecar 纯函数）

---

## 一、背景

62 号清单 play 域 4 条是互动世界游玩的读取与插图面：游玩页聚合（transcript + 当前状态 + 世界 + 事件图谱快照 + 插图合并）、插图开关、按需生图、插图文件回读。写入面（世界创建/回合推进）经 64 号 play 会话与 67 号 play_start 意图入口在 agent 域，本号补齐游玩读取面。

## 二、交付

### 1. 域本体 `engine-rs/src/play.rs`（替换 2 行占位，~560 行 + 6 单测）

- **路径安全**：`is_safe_segment` / `world_dir` / `run_dir`（worlds/{id}/runs/{id}；斜杠/反斜杠/NUL/`.`/`..` 拒绝）+ `safe_run_child_path`（投影相对路径防逃逸）。
- **PlayStore 读取面**：`read_transcript`（transcript.jsonl 逐行宽松——坏行/空行跳过，zod 形态校验 role/content/timestamp）/ `load_current_state`（state/current.json 坏→None）/ `load_world` / `read_projection`。
- **图谱快照（createPlayDB(runDir).snapshot() 的读取等价）**：
  - **sqlite 主后端**：`play.db` 存在 → rusqlite 只读打开，四表 SELECT（列名/别名/JSON 列解析逐字对齐 play-db.ts——entities 七列、edges 十一列含 value_json/visibility_json/strength/confidence、state_slots 六列、events 六列）；
  - **文件回退**：`play-graph.json` 四 record 逐项宽松（parseRecord safeParse 失败跳过）；
  - 排序：entities/edges/slots by id、events by (turn, id)；无后端 → 空四数组。
- **插图 sidecar**：manifest 读写（坏→空）/ settings 三开关读写（默认全关；Boolean 强转）/ `play_image_file_name`（折叠+80 码元）/ `build_play_entity_image_prompt` + `build_play_scene_image_prompt`（八类实体镜头语 + 世界三元组渲染行 + 400/600/700/900 UTF-16 截断逐字）。

### 2. 端点 `engine-rs/src/server/play_routes.rs`（4 条）

- `GET /play/runs/:worldId/:runId`：五源聚合（transcript + currentState + world.title + graph 快照 + 插图 sidecar）——**ready 实体注入 `imageUrl`**（failed 不注入）、`scene-turn-*` 收集为 URL 表、当前回合插图 `sceneImageUrl`（turn 取 currentState）、imageSettings。段归一：非法 400 `INVALID_BOOK_ID`（normalizeApiBookId 同码）。
- `PUT .../image-settings`：三开关 Boolean 强转覆写 → `{ok:true, imageSettings}`。
- `POST .../generate-image`：entity/scene 双目标——entity 走图谱查找（缺失 404 `entity not found`）+ 类型镜头提示词；scene 默认投影文本兜底（无 → 400 `no current scene to illustrate`）+ `scene-turn-{turn}` 键。**生图执行链未移植**：与 TS "cover API 未配置" catch 分支同形兜底 `{error, needsCoverConfig:true}` 400（偏差备案）。
- `GET .../images/:file`：png/jpg content-type 回读；`/`、`..`、NUL → 400 `Invalid image file`；缺失 404。

### 测试

- **lib 单测（6 个）**：路径安全正反例（含 `a/../../escape` 深层逃逸）、transcript 坏行跳过、文件后端快照（坏项剔除 + events (turn,id) 排序 + 空目录四空数组）、**sqlite 后端四表读回**（NULL 列/JSON 列/strength）、settings roundtrip 与 manifest 坏文件、提示词构建（世界三元组行/类型镜头/截断省略号）与文件名折叠。
- **E2E `play71_e2e`（4 个）**：run 聚合全量断言（title/transcript/currentState/实体 ready 注入与 failed 不注入/sceneImageUrls 表/当前回合 sceneImageUrl/默认开关）；**sqlite 后端端到端**（造 play.db → graph 从 sqlite 读）；settings 写后落盘回读 + generate-image 四分支（缺 entityId 400 平铺/实体 404/needsCoverConfig 兜底/scene 无文本 400）；图片回读（content-type + 下载体 + 穿越拒绝 + 缺失 404 + 非法段 INVALID_BOOK_ID）。

## 三、parity 要点

1. **后端优先级**：TS createPlayDB sqlite 可用即 sqlite（不看 json）——Rust 同序（play.db 优先，损坏则回退 json）。
2. **快照排序语义**：四集合稳定排序（id / turn+id），前端图谱渲染顺序一致。
3. **插图合并是纯展示层**：manifest 与事件图谱解耦（sidecar），failed 条目不注入 URL；当前场景插图按 currentState.turn 命中。
4. **段归一同码**：worldId/runId 非法复用 normalizeApiBookId 的 INVALID_BOOK_ID（E2E 固化）。
5. **scene 文本兜底链**：body.sceneText → projections/scene.md → 400。

## 四、偏差备案

1. **生图执行链未接线**：generate-image 返回 `{error, needsCoverConfig:true}` 400——与 TS "cover API 未配置" 分支同形，前端走相同配置引导路径；提示词构建已逐字就绪，生图基础设施（cover-generation）移植后一行接上。
2. **写入面不在本号**：createWorld/updateWorld/saveCurrentState/appendTranscriptTurn/graph 变更（play-reducer）与 play-runner（回合 LLM 执行流）——现有端点消费面只读；写入入口经 agent 域 play 意图（67 号 play_start 仍为 Unsupported，域本体就绪后接线）。
3. **file 快照的 zod 校验为关键字段子集**（必填字段形态而非全 default 回填）——展示用途等价，写回形态差异无消费方。
4. **sqlite 打不开（损坏/锁）静默回退 json**——TS 构造失败会 throw（端点 500）；Rust 宽容回退，读取面更稳。

## 五、暂缓件（沿 70 号清单滚动）

- 62 号清单剩余：daemon/doctor/logs/radar 7 条、foundation/revise 1 条（缺口 8 条）
- play 写入面与 play-runner 回合执行流 + play_start 确认意图执行器接线
- 生图链（cover 基础设施）+ pdf 源 + film-authoring LLM 链 + draft_structure/connect_choice/remove_node 执行器
- 单章写作中途截断、actionPayload strict、/agent model 校验、resumeFrom、fetchWithProxy、attachments、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1015** 通过（+6） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **111** 通过（+4） |
| `cargo test --features export-bindings --lib` | 1174 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：互动世界游玩页（状态 HUD/图谱/插图）读取面可切流；62 号清单缺口 12 → 8 条。
- **下一步（72 号候选）**：daemon/doctor/logs/radar 运维面 7 条 + foundation/revise 1 条（57 号暂缓的 reviseFoundation 四文件装配）收官，62 号清单清零。
