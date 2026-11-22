# 1658 号：R54 prompt 版本化与回归门——引擎面全量（版本账本+历史/回滚端点+regressionHint）

> 日期：2026-11-23 ｜ 类型：功能轮（v8 深化三 R54 落地第一档；1647 修正路径核实：覆盖面=`{root}/prompt/<dirs>/<name>.md`（prompt_override_path 点分 id 映射），非 `.inkos/prompts/`）
> 定位：prompt-packs 端点族**Rust 在位**（GET/PUT/DELETE+e2e；skill_routes 注释所指 TS server 面已失联——studio 零 prompt UI/端点，**R54 本轮=引擎面全量，UI 面挂下轮**（studio 无 prompt 基础面从零起独立轮））。

## ① prompt 覆盖版本账本（`{root}/prompt/.versions.jsonl`）

- `prompt_pack.rs` 版本账本段（沿 writing-log 惯例）：append-only 行 `{ts, promptId, action: put|delete|restore, content}`；读旧+拼+原子写；读取宽容坏行跳过；**best-effort**（追加失败 eprintln 吞不 fail 覆盖写主链）。
- `load/append/list（promptId 过滤+ts 降序）/read（promptId+ts 定位）` 四函数。

## ② 挂点与新端点（skill_routes）

- **PUT 挂点**：覆盖写成功后 append（action=put, content）；**DELETE 挂点**：移除后 append（action=delete, content 空——回滚到 delete 前版本可恢复）。两响应加 **`regressionHint: "prompt-changed"`**（v8「评测回归门」轻形态：消费端提示跑 R8 行为评测；评测本体挂观察——builtin 变更守卫由既有 golden_agent_prompts_diff 家族承载）。
- **GET `/prompt-packs/:promptId/versions`**：列表（ts 降序；行={ts, action, preview(前 80 字), content}；无账本=空数组）。
- **POST `/prompt-packs/:promptId/versions/restore`**（body `{ts}`）：读版本行→非 delete 行复写覆盖文件（同 PUT 原子写）/delete 行=移除覆盖回 builtin → append（action=restore）——**回滚本身也是一次可再回滚的变更**（append-only 语义闭环）；响应 prompt+regressionHint。
- 负路径：非法 JSON/缺 ts→400 INVALID_PROMPT_VERSION_PAYLOAD；未知 ts→400 PROMPT_VERSION_NOT_FOUND（`{"error":{"code","message"}}` onError 形状）。

## ③ e2e 全链（skills60 族）

- `prompt_pack_versions_append_list_and_restore`：PUT v1（hint 断言）→GET versions 1 行→PUT v2→2 行降序（最新在前）→restore v1→盘上覆盖文件=v1 内容+3 行（restore 首）→未知 ts 400；既有 PUT/DELETE roundtrip 测随 hint 字段自然兼容（响应加键不断言不破）。
- 错误体形状勘误：`parsed["code"]`→`parsed["error"]["code"]`（api_error onError 嵌套形状）。

## 范围与备案

- **TS/studio 零面实证**：server.ts 无 prompt-packs 端点、无 prompt UI 组件——R54「UI prompt 面列版本历史+diff+一键回滚」挂下轮（studio prompt 基础面+版本 UI 一体起）；TS 引擎侧（packages/core）亦无此域（60 号移植时该面未入 core）。
- **R8 行为评测锚点自动跑**：挂观察（需 LLM 运行时=非门禁可常驻；v8「本地门禁形态」的完整形态=R55 同批评估）；本轮以 regressionHint 通道+builtin golden 守卫承载「接入先测」纪律的机制面。
- 三级覆盖（builtin/project/user——LoadPromptPackPromptInput 双 root）读取语义既有；版本账本记 project 覆盖写点（user 级覆盖写面引擎端无端点）。

## 门禁记录

- cargo:testgate 全绿：audit 双 0 + 必红自检活体（9+1）+ engine-rs **69 目标 2194 passed**（+1=e2e 版本化全链测）+ src-tauri 600；INKOS_DUEL=1 真跑。
- gate:ts 十腿全绿（本轮零 TS 变更终验）。
- fmt（--check 幂等）+ clippy --all-targets 双 0；U+FFFD 净。

## 出口

- R54 引擎面收口；下轮=R54 UI 面（studio prompt 基础+版本历史/diff/回滚）或 R55 用量账本。
- 下号自 1659 起；出口=R54 UI/R55 用量预算警戒（先立 per-service 账本）+观察项巡检。
