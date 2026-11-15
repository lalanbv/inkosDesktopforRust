# 1429 号：逐功能审查双盲区修复轮——分类器裸码锚定 + schema 遗留键双栈漂移

## 目标与范围

/goal 逐功能测试新增代码逻辑+代码审查+模拟真人测试。对象=1426/1427/1428 三轮新增面
（启动门分类器+App 错误门分流、裸网关 5xx 锚定、ChatPage/Sidebar IME 交互测试），
零已知 bug 输入下的审查轮。产出=两处真盲区修复（修复 A 分类器裸码、修复 B schema
遗留键）+活体三腿第一手实证。

## 逐功能审查结论（1426/1427/1428 面）

- 分类器 `isBackendUnreachableMessage`：四组单测覆盖到位（引擎正例/锚定负例/可达
  反例/空值安全）；`readErrorMessage` 回退串经 `localizeKnownRuntimeMessage` 透传
  （映射表无匹配不变形）——分类器输入无加前缀风险 ✔；正则无 `g` 旗标无 lastIndex
  态 ✔（854 族教训自查）。
- App 错误门分支：纯条件渲染，判定逻辑已被分类器单测覆盖，分支渲染由本轮活体三腿
  第一手复核 ✔。
- 1428 两份交互测试：mock 形/flush 纪律/radix jsdom 桩形态良好，无缺陷 ✔。
- deriveStartupGate 纯函数既有 3 腿单测 ✔。
- i18n 台账 App.tsx 预算 5 与实况一致（1426 分流无新增硬编码）✔。

## 修复 A：分类器裸状态码盲区（1427 同族残余）

**形态**：`readErrorMessage` 回退串 `` `${status} ${statusText}`.trim() `` 在
statusText 为空（HTTP/2 响应无 reason phrase、不回 reason phrase 的裸代理）时恰为
裸 `"500"`——1427 整串锚定 `^5xx (phrase)$` 不命中 → 仍误指 inkos.json。

**修复**：`BACKEND_UNREACHABLE_PATTERN` statusText 段改可选
`^5(00|02|03|04)( (Internal Server Error|Bad Gateway|Service Unavailable|Gateway Timeout))?$`——
裸码与带短语两形同锚定；带详情/前后缀不误判的分流边界不变（5001/带 details/HTTP/1.1
前缀/404/403 全负例复验）。

**测试**：新腿经 `fetchJson`→`readErrorMessage`→`localizeKnownRuntimeMessage` 真链路
生产派生（`new Response(null, {status})` 即 statusText=""，正是无短语响应的浏览器视
角；1007 契约纪律防手打双漂假绿）。use-api.test.ts 单跑 8/8。

**活体边界实证**：vite dev 的 http-proxy 会**补回标准 reason phrase**（curl 实采
`HTTP/1.1 500 Internal Server Error`）——裸码形态不经 vite 链产生，属防御性收口
（HTTP/2 中间层/裸代理面）；活体腿A 用 raw socket 裸 500 夹具验证了整链分流正确。

## 修复 B：ProjectConfigSchema 拒收 inputGovernanceMode（686 遗留键双栈漂移，活体抓虫）

**发现链**：活体自愈腿起 TS dev:server :4569 → `Failed to start studio: ZodError
unrecognized_keys: inputGovernanceMode` 拒启。定性：

- inkos.json（仓根，未跟踪，**8/6 起在位**）含 `inputGovernanceMode: "v2"`；
- 686 号随 TS 上游 e7c04465 删除 legacy 输入治理管线，Rust 侧
  （engine-rs project_config_routes.rs 头注）对齐备案「该键**双端同为未知键忽略**」；
- 但 TS `ProjectConfigSchema` 为 strict 根（models/project.ts），实际拒收且
  ZodError **裸抛未经人话包装**——与契约注释漂移：同一份 inkos.json Rust 引擎 200、
  TS 腿 loadProjectConfig 必挂；
- 影响面=TS dev 工作流 + 桌面 Node 回退腿（164 号 rustbin miss 时回退）；
  1427 自愈腿起的是 Rust 引擎占 4569，故此前从未暴露（修复前 Retry 拿到的是代理
  500 不可达门，修复后同一按钮走回仪表盘——完美对照）。

**修复**：沿 709 号先例（Rust 落盘键毒化 TS strict 根同族，`writing` 段同型收敛）
=schema 单源容键 `inputGovernanceMode: z.unknown().optional()`——
effective-llm-config.ts:134 与 short-fiction-runner.ts:1437 两 parse 点同获覆盖，
strict 根设计不动（防手误护栏保留，`someFutureLegacyKey` 仍拒），
z.unknown 不冻结已死管线历史值域；写回面直读 raw JSON 不经 schema=零写面副作用。

**测试**：config-writing-keys.test.ts 新腿三断言（仓根实采键形 parse 绿+值透传/
任意历史值容忍/缺省省略不变+其余未知键仍拒）。core 单文件 4/4。

## 验证谱

- core 全量 187 文件 **1069/1069**（+1）；studio 全量 167 文件 **910/910**（+1）；
- typecheck：studio 双 tsconfig 绿 + core tsc 绿；build:client 绿；
- 烤入面：前端新 bundle `index-b9PIdL30.js` rsync 两处（src-tauri/engine-rust/static
  + src-tauri/target/release/engine-rust/static），新正则形活体 grep 各 1/1；
  core dist 重烤（grep inputGovernanceMode=3）——Node 回退腿 dev 形 symlink 与
  PROD 打包 deploy 同源 `packages/core/dist` 即吃到修复；
- 浏览器活体三腿（本会话第一手）：①raw socket 裸 500（无 reason phrase）夹具→
  vite :4567→错误门分流「引擎不可达」双语指引、inkos.json 误导指引未现；②真
  ECONNREFUSED（:4569 无服务）→代理 500→同分支（1427 场景回归）；③点重试/Retry→
  **修复后** TS 服务 200→完整仪表盘渲染（侧栏/工具/会话全在位）=自愈链端到端；
- U+FFFD 四文件全净；清场 vite(:4567/:4568)/TS server(:4569)/raw500(:4599)/走查
  标签页全关，端口归零，临时夹具删除。

## 边界定性备案

- `src-tauri/engine-rust/static/{api,lib,shared}` = build:server 产物（tsc 依赖链
  把 studio src/lib、src/shared 一并编进 dist），系 1427 时点 rsync 带入的**静态面
  死重**——Rust 引擎 serve 静态文件不执行 JS，生产 Rust 形态无消费；下次正式打包
  desktop-package-rust-engine.sh 的 `rm -rf $DEST` 自会清除。本轮不动（无执行面
  漂移风险）。
- vite dev 的 http-proxy 补回 reason phrase → 裸码形态防御性收口（见修复 A）。
- 用户现有二进制的 Node 回退腿仍是旧 schema：与 1427 同备案，**用户侧待办=正式打包**
  （desktop-build-inkos.sh + desktop-package-rust-engine.sh + desktop-package-engine.sh）
  后 Node 回退腿才吃到修复；Rust 直启默认形态无此 bug 不受影响。

## 教训

1. **「双端同为未知键忽略」类契约注释必须双端行为级验证**——Rust 侧备案了契约，
   TS strict 根静默违背，8/6 至今无人踩（Node 腿低频+1427 活体恰好用 Rust 引擎
   占位）。契约差分器对「注释宣称 vs 实现行为」是盲区，活体真服务起服是唯一捕获面。
2. **数字里程碑审查轮的活体腿必须起「真服务」而非复用上轮端口占用者**——1427 用
   Rust 引擎占 :4569 让「起真引擎」成真，但 TS 腿的对称故障被端口语义掩盖；本轮
   显式起 TS server 才现形。同端口语义下的双栈对称腿（Rust 起=1427、TS 起=本轮）
   应成活体惯例。
