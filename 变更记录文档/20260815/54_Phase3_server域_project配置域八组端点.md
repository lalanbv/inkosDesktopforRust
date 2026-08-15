# 54 号变更记录：Phase3 server 域——project 配置域八组轻量端点（inkos.json 读写面）

- 日期：2026-08-15
- 范围：engine-rs（新增 `server/project_config_routes.rs`；扩展 `server/mod.rs`）
- 契约源：`packages/studio/src/api/server.ts` L4348-L4388（input-governance-mode/detection）、L5553-L5566（language）、L5733-L5815（model-overrides/default-model/research-search/chapter-review-mode）、L5875-L5888（notify）；Schema：`packages/core/src/models/project.ts`（DetectionConfigSchema/InputGovernanceModeSchema/ResearchSearchConfigSchema）
- 验证：`cargo test --lib`（925）+ `cargo test --test golden_leaf`（76）+ `cargo test --test e2e_write_next_contract`（40）+ `cargo test --features export-bindings --lib`（1084）+ `cargo clippy --lib --tests --bins`（零警告）+ TS vitest（185 文件/1798 测试）

## 一、背景

53 号后配置域是剩余轻量面。勘测后定案：八组键值端点全量移植；`GET /project`（全量配置读写）与 `POST /default-model` 的 `syncTopLevelLlmMirror` 顶层镜像暂缓——后者依赖 40+ provider 预设表（`core/llm/providers/endpoints/*` 目录 30+ 文件），属 LLM provider 配置域大件。

## 二、交付内容（`server/project_config_routes.rs`，~430 行，15 端点）

| 端点组 | 行为要点 |
|---|---|
| `GET/PUT /project/input-governance-mode` | 读：`=== "legacy"` 判 legacy 否则 v2；写：非 legacy/v2 → 400 `"mode must be legacy or v2"` |
| `GET/PUT /project/detection` | DetectionConfigSchema 校验 + **zod default 填充**（7 字段标准化：provider=custom/threshold=0.5/enabled=false/autoRewrite=false/maxRetries=3；未知键剥离）；错误 400 拼接形态 `msg1; msg2`；null 删键；缺 detection 键 → 400 |
| `GET/PUT /project/model-overrides` | 对象透传；缺省读 `{}`；body 缺 overrides → 赋 undefined → 键删除 |
| `GET/PUT /project/default-model` | GET：llm 非对象视 {}，defaultModel（trim 非空）→ model 链式回退；PUT：defaultModel 必填 400、llm.defaultModel + 可选 service 写入（sync 镜像暂缓） |
| `GET/PUT /project/research-search` | Schema 校验 + 填充（整体缺省 `{enabled:false, provider:"tavily"}`；可选 baseUrl（URL 校验）/apiKey/apiKeyEnv）；解析失败 → zod 抛 → 500 onError |
| `GET/PUT /project/chapter-review-mode` | project 级 writing.reviewMode（仅 "manual" 精确命中，其余归 auto；保留 writing 其他键） |
| `GET/PUT /project/notify` | 数组透传；缺省读 `[]`；缺 channels → 删键 |
| `POST /project/language` | language 值**不校验透传**（undefined → 删键 + 响应无 language）；端点自带 catch → 500 平铺 error（区别 onError 形状） |

共享件：`load_raw_config`（inkos.json 读，保未知字段）、`save_raw_config`（`to_string_pretty` = 2 空格缩进无尾换行，对齐 `JSON.stringify(raw, null, 2)`）、`is_valid_url`（zod `z.string().url()` 简化等价：scheme + `://` + 非空余部）。

## 三、parity 要点

1. **zod default 填充语义**：detection/research-search 校验成功后落盘的是**填充后的标准化对象**（非原样透传）——未知键剥离
2. **JS undefined 赋值删键**：model-overrides/notify 的 `raw.x = undefined` 在 stringify 时键消失——Rust 以 remove 对齐
3. **language 不校验**：任意 JSON 值（含非字符串）透传落盘
4. **错误形状三态**：zod 校验失败 400（issues 拼接）/ 端点 catch 500 平铺（language）/ 无 catch 500 onError 形状（其余全部，`{"error":{"code":"INTERNAL_ERROR",...}}` 逐字）
5. research-search 的解析失败是 500 而非 400（`Schema.parse` 直接抛走 onError）——与 detection 的 `safeParse` 400 形成对照，server.ts 原文如此

## 四、偏差备案

1. zod 校验错误文案：Rust 为英文近似（"Invalid url"/"Number must be less than or equal to 1" 等），状态码与拼接形态（`; ` 连接）一致
2. `PUT /default-model` 不做 `syncTopLevelLlmMirror`（llm.model/baseUrl/provider 顶层镜像）——落盘少这三个镜像键；依赖 provider 预设表，随 LLM provider 配置域移植后补
3. 落盘 JSON 键序：serde_json Object 为 BTreeMap（字母序），TS JSON.stringify 保插入序——功能等价（JSON 键序无语义）但 diff 噪声不同（48 号起既有行为，此处统一备案）
4. `is_valid_url` 简化：不接受 `mailto:` 类无 authority 的 URL（zod 的 url() 接受）；配置场景（apiUrl/baseUrl 均为 http(s)）无实际影响

## 五、暂缓件

- `GET/PUT /project`（L4181/L4324 全量配置读写——ProjectConfigSchema 全字段校验，随 architect/qualityGates 等配置消费方一并）
- `POST /default-model` 的 syncTopLevelLlmMirror（provider 预设表大件）
- `GET /project/files|artifacts` 与 `PUT /project/artifacts`（文件浏览面）
- `projects/:id/story-graph|export|nodes` 域（故事图谱可视化，独立大域）

## 六、下一步（55 号候选）

1. import/fanfic 域端点（generateStyleGuide/style-analyze/importBook）
2. `GET/PUT /project` 全量配置 + qualityGates/foundation/writing 子配置
3. Node sidecar 下线核对：books 42 + genres 6 + project 配置 15 = **63 端点**已可切换
4. books/create（architect 域大件）

## 七、影响面

- Rust 业务端点累计：46（53 号后）+ 15（8 组）= **61 个**
- 新增测试：e2e +4（config54_e2e）；lib 无新增（纯装配 + 内联校验器，行为由 E2E 覆盖）
- 无破坏性变更；project/* 路由族与既有 books/genres 无冲突
