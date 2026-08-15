# 69 号：interactive-films / projects 域 —— 故事图谱与三导出

**日期**：2026-08-15
**阶段**：Phase3 strangler 迁移（62 号清单清缺口：interactive-films/projects 域 10 条——最大缺口块）
**契约源**：`packages/studio/src/api/server.ts` L6504-L6812（10 端点）+ `packages/core/src/interactive-film/`（16 模块 1203 行：graph-schema / graph-store / delta / evaluator / paths / emotion / validation / export-ink / export-html / node-image / authoring-store）

---

## 一、背景

62 号下线核对清单中 interactive-films/projects 域 10 条是最大缺口块（54 号暂缓件）。该域为独立可视化域：故事图谱（节点/选项/变量/结局）的存取、增量变更、14 规则校验、路径与情感分析、三种导出（Ink 脚本 / 可玩 HTML / tar.gz 归档）。域本体纯逻辑无 LLM 依赖（生图除外），可整域一次交付。

## 二、交付

### 1. 域本体 `engine-rs/src/interactive_film.rs`（替换 2 行占位，~1150 行 + 11 单测）

- **schema（graph-schema.ts 逐字段）**：VarValue 联合（自定义反序列化拒绝 null/数组/对象）/ Condition（6 比较算子 serde 字面量）/ Effect（set/add/sub）/ Choice（targetNodeId + 条件 + 效果 + 权重）/ DialogueLine / ImageSlot / NodeType（6 值）/ VoiceProfile / Character（role 默认 other）/ WorldAnchor / StoryNode / Variable / Ending / StoryGraph（schemaVersion literal 1 解析后校验）。zod `.default()` → `#[serde(default)]` 逐字段；**全结构 `rename_all = "camelCase"`**——磁盘文件为 TS 写的 camelCase（schemaVersion/projectId/targetNodeId/nodeId/worldAnchor…），开发中单测捕获了 snake_case 命名会导致读写 TS 文件全错的 parity bug。
- **graph-store**：`load_story_graph`（缺失 None / 坏 zod Err——列表 skip、端点 500 的语义分叉）/ `save_story_graph`（pretty + 尾换行）。
- **delta**：`apply_story_graph_delta`（upsert/remove 四集合 + worldAnchor 浅合并 + **ending 引用完整性 Err**）。
- **evaluator**：条件求值（JS `Number()` 强转语义）/ 效果应用 / 可见选项 / 变量初值。
- **paths**：`enumerate_runtime_paths`（DFS + (node, varState) 去环 + 200 路径/50 深度上限；ending 与死端都记终态）。
- **emotion**：20 词情感词典 + **否定字符翻转**（"不高兴"→负效价）+ 逐节点均值弧线 + 按结局/长度分布。
- **validation**：`validate`（BROKEN_LINK/DEAD_END/UNREACHABLE/NO_PATH_TO_ENDING）+ `review` 十项评审（VARIABLE_UNWRITTEN/VARIABLE_UNUSED/ENDING_VARIETY/IMAGE_MISSING/GATED_UNREACHABLE/ENDING_UNREACHABLE——**路径枚举截断时跳过不可达断言**（TS 注释逐字）/LINEAR_GRAPH/ISOLATED_NODE（跳过已报 UNREACHABLE 的节点防重复）/ILLUSORY_BRANCH/LONG_LINEAR_CHAIN（≥5 链头））。
- **export-ink**：sanitize（非字母数字折叠 `_` + 数字头补 `n_`）+ VAR 声明 + knot + 选项条件/效果 + `-> END`。
- **export-html**：PLAYER_JS/CSS 原样内嵌 + `<` 转义防 `</script>` 逃逸 + 标题 HTML 转义。
- **node-image 路径面**：`node_image_rel_path`（encodeURIComponent 严格集 + `!'()*` 补充转义）。

### 2. 端点 `engine-rs/src/server/interactive_film_routes.rs`（10 条）

- `GET /interactive-films`：目录枚举 + safe id 过滤 + 坏图谱 skip + 标题排序（码点序——zh 拼音序需 ICU，偏差备案）。
- `POST /projects/:id/story-graph/delta`：**authoring-store 逐字**——per-project 异步互斥（withProjectLock 防丢更新）→ 加载（缺失回空图谱）→ **pre-rev 快照**（保留最近 20）→ 应用 → 保存 → rev+1 落 authoring-state.json；响应 `{rev, graph}`。
- `GET /projects/:id/story-graph`：**原样回显**（不做 zod 校验——与 loadStoryGraph 的差异语义）。
- `GET /projects/:id/export`：tar.gz（**ustar 头逐字段**：100 名/8 mode/12 size/12 mtime/checksum 空格填充/ustar magic；512 块 + padding + 1024 尾；.DS_Store 过滤 + symlink 兜底 stat；flate2 gzip）。
- `GET .../validation` / `.../analysis`：review 报告 / report+arcs+distribution 三段。
- `GET .../export/json|ink|html`：json（pretty+尾换行）/ ink / html（资产 data URI 内嵌，坏资产跳过；复用 61 号 resolve_project_image_file）+ attachmentDisposition（ascii fallback + UTF-8 编码双 filename）。
- `POST /projects/:id/nodes/:nodeId/image`：节点存在性 404 / 生图链未移植 503 `IMAGE_GENERATION_UNAVAILABLE`（偏差备案）。

### 3. 依赖与接线

Cargo.toml + flate2 / base64；`resolve_project_image_file` pub(crate) 化（61 号）；mod.rs 注册 `interactive_film_routes` + router_books 挂 10 条路由。

### 测试

- **lib 单测（11 个）**：schema 默认填充与坏值拒绝（VarValue null / schemaVersion literal）、delta upsert-remove 与悬空 ending、条件求值 JS 强转、路径枚举（门控分支/无 start）、情感词典与否定翻转、validation 各规则（断链/死路/无结局/VARIABLE_UNWRITTEN/ILLUSORY_BRANCH/LONG_LINEAR_CHAIN）、export-ink 形态、html 转义、store roundtrip 与坏版本、node 路径编码。
- **E2E `films69_e2e`（5 个）**：列表（排序/无效目录过滤）+ delta 两次（rev 1→2、快照 0/1 存在且 2 不存在、authoring-state 落盘、回显）；INVALID_ID 与悬空引用；validation/analysis（坏图 ok:false 三错误码、完好图三段形态、404）；四导出（ink 文本与头、html 资产内嵌、json pretty、tar.gz gunzip 后 ustar magic + 首条目名）；node image 面（503/404）。

## 三、parity 要点

1. **磁盘 camelCase**：TS 写的 story-graph.json 字段全 camelCase，Rust schema 必须 rename 对齐（单测固化 schemaVersion literal 拒绝）。
2. **load 与 GET 的校验差异**：loadStoryGraph 走 zod（坏文件 Err/500/skip），GET story-graph 原样回显——两条路径不同语义。
3. **delta 的 rev 链**：pre-rev 快照（活文件即最新 rev 不快照）+ per-project 锁防并发丢更新。
4. **校验的健全性守卫**：路径枚举截断（>200）时 GATED_UNREACHABLE/ENDING_UNREACHABLE 跳过（不完整集合上断言不可达不健全）。
5. **tar 头 checksum**：先空格填充再求和再回写（TS 逐字段）。

## 四、偏差备案

1. **生图链未接线**：`POST nodes/:nodeId/image` 生产返回 503（short-fiction runner 的 cover 生成基础设施未移植）；生成 + setImageRef delta 逻辑随后续号接线（路径面 node_image_rel_path 已就绪）。
2. **zh 排序降级**：列表 title 排序用码点序（TS localeCompare zh 拼音序需 ICU）——同名首字不同拼音场景顺序有差。
3. **upsert 输出序**：BTreeMap 键序 vs TS Map 插入序——中段插入新项的相对顺序有差（前端按 id 索引无感）。
4. **draft_structure / connect_choice / remove_node 三个确认意图执行器未接线**：draft_structure 依赖 film-authoring LLM 链（authoring-generate/tools）；connect_choice/remove_node 的 delta 语义已就绪，接线随后续号（67 号偏差备案滚动）。
5. **authoring-store 的 revertToSnapshot / recordPhaseVisit / phase 传递**：TS 有而端点未消费的面，未移植（端点契约只走 delta 的 rev+phase 保持路径）。

## 五、暂缓件（沿 68 号清单滚动）

- 62 号清单剩余：translations 6 条、play 4 条、daemon/doctor/logs/radar 7 条、foundation/revise 1 条
- 生图链（cover 生成基础设施）+ node image 端点接线
- film-authoring LLM 链（authoring-generate/tools/context/memory-link）+ draft_structure 等三个确认意图执行器
- 67/68 号遗留：单章写作中途截断、actionPayload strict、/agent model 校验、configuredEntry 深链、resumeFrom、fetchWithProxy、attachments、模型四层解析

## 六、验证基线

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **1001** 通过（+11） |
| `cargo test --test golden_leaf` | 76 通过 |
| `cargo test --test e2e_write_next_contract` | **104** 通过（+5） |
| `cargo test --features export-bindings --lib` | 1160 通过 |
| `cargo clippy --lib --tests --bins` | 零警告 |
| `packages/core vitest run` | 185 文件 / 1798 测试全绿 |

## 七、影响面与下一步

- **影响面**：互动影游前端（图谱编辑器 / 校验面板 / 分析面板 / 导出菜单 / 项目归档下载）全部可切流到 Rust 端点；62 号清单缺口 28 → 18 条。
- **下一步（70 号候选）**：translations 域 6 条（upload/create/list/detail/run/export——翻译工作流，域本体 translation/ 目录已存在 Rust 骨架）或 play 域 4 条（互动世界）；随后 daemon/doctor/logs/radar 7 条运维面收官。
