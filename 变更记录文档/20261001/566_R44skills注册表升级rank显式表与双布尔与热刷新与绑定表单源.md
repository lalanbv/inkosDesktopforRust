# 566 号：R44 skills 注册表升级——rank 显式表+invocation 双布尔+skills:change 热刷新+绑定表 JSON 单源

- 日期：2026-10-01
- 类型：feat(engine+core+studio)
- 路线：dsh 对标 v7 P2 首项 R44（541 号 §P2/§表 R44；544 号明言 R44 未深化待立项出施工图——本档即施工图+实施）
- 状态：✅ 门禁全绿收口

## 交付面（四件）

### ① rank 显式表替代隐式目录序（双端）

- dsh 语义（541 号源码级证据 L23）：分层注册表 rank 裁决重名（项目 100 → 用户 600 六级，低者胜）。
- **装载层缺省六级表**（低者胜，与原装载序隐式优先逐层等价——默认零行为变更）：project `skills/`=100 → project `.agents/skills/`=200 → `~/.agents/skills/`=300 → `~/.openclaw/skills/`=400 → env `INKOS_SKILL_DIRS`=500 → builtin=600。由 loader 按**装载层**（目录级）注入——不能用 source 枚举（project 两目录异层、user 两目录异层）。
- frontmatter `rank`（0–1000 整数）显式覆盖，可跨层升降（user 技能 rank=50 可压过 project 缺省 100）。**严格面**：在场但非法 → 整份 SKILL.md 诊断拒绝（防位置数据静默回落缺省层）；治理布尔维持 R37 宽松先例（仅精确布尔生效）——刻意不对称，备案。
- registry 同 id 决胜=有效 rank 升序、平秩后写胜；rank 缺省（手工构造）退化为纯后写胜=R44 前行为（130 号同名覆盖语义在平秩分支保持）。
- golden：`skills-registry-vectors.json` 8 case 双端首跑即绿（TS skills-registry.test.ts 9 断言+Rust tests/golden_skills_registry_diff.rs）。

### ② invocation 双布尔第二位 `user-invocable`（双端）

- dsh 同名语义，缺省 true；`false`=用户面不可见（GET /api/v1/skills 过滤——Rust skill_routes+TS loadStudioSkills 源列表过滤），模型面 resolveSkills 不受影响。与 R37 `disable-model-invocation` 正交，四组合保留。
- **修自摆乌龙一例**：过滤初版写在端点响应层（StudioSkill 映射产物无 userInvocable 字段→恒真过滤失效）——改到 loadStudioSkills 源列表（映射前）。教训：过滤必须作用于持有判别字段的层。
- 测试：TS skills-endpoint.test.ts（+1，7/7）+Rust e2e skills60（list_skills_hides_user_invocable_false，9/9）。

### ③ skills:change 失效事件 → SSE → studio 热刷新

- 命名房规冒号风格 `skills:change`（v7 写 `skills/change` 为拼写偏差，备案）；STUDIO_SSE_EVENTS+studio-sse-events.json golden 同批更新（该 golden 拦截当场红=护栏在工作）。
- 发射点双端三处：①聊天轮成功执行 author_skill（Rust agent_route 查 tool_executions+TS hasSuccessfulToolExec；R37 写链的热载通道=541 号 §零重叠声明兑现）②skills import ③skills delete。payload `{reason: authored|imported|deleted, sessionId?}`。
- studio 消费：`useSkillsInvalidation` hook（游标消费语义）+ChatPage（既有未用 `sse` prop 启用）+ProjectSettings（新增 sse prop，App 单流传入不开新连接）。
- Rust 发射点无 hub 于 ToolCtx（仅 root）→ 放路由层（runtime.hub 在场），零工具签名变更。

### ④ production_bindings 八能力绑定表迁 JSON 单源

- `packages/core/src/skills/production-skill-bindings.json`（值即文件）；TS `import ... with { type: "json" }`（NodeNext 强制 import attribute——实证 TS1543+dist 纯 node 加载验证）+Rust `include_str!`+OnceLock 解析。双端零漂移 by construction（555 号活体硬对照精神：值即文件）。
- ProductionSkillCapability 类型派生链改造：TS 原从 const 推导改为显式联合（Rust 枚举同族+Hash derive 供 HashMap）。
- 测试：TS production-skill-bindings.test.ts 4/4（值面不变自然绿）+Rust bindings_json_loads_all_capabilities（八能力逐项断言）。

## 门禁

- clippy:gate 双 crate 0 告警
- cargo:testgate：engine-rs 45 目标 1892 passed（+1 golden_skills_registry 目标）+src-tauri 23 目标 576
- gate:ts 七步全绿（studio 901 测试含新 hook 交互 1+endpoint 7；core 含 registry golden 9+loader 15）
- verify:engine-bindings 187 出口全绿（AgentSkill 新字段 ts_rs 导出面同步）

## 教训

- 分层缺省表必须键于**装载层**（目录级）而非 source 枚举——枚举粒度不足以区分同源异层目录。
- 过滤/投影类改动核对「字段存活在哪一层」：映射后再过滤=恒真空转。
- NodeNext ESM 消费 JSON 必须 `with { type: "json" }` import attribute（TS1543 编译期拦截+运行时 ERR_IMPORT_ATTRIBUTE_MISSING 双面），dist 纯 node 加载需实测。
- STUDIO_SSE_EVENTS golden 快照（549 号护栏）当场拦截新事件——R31 面锁工作正常，新 SSE 事件三件套=常量表+golden+broadcast 面同批。

## 关联

- 前序：537（production_bindings 活体面）、559/560（R37 disable-model-invocation 治理面+写链）、549（SSE golden 护栏）
- 路线：v7 P2 剩 R45（LLM/subagent provider seam 化）、R46（WASM 插件宿主契约收敛）按需立项
- 下一号自 567 起
