# 635 号：SSRF 防线覆盖面审计收官——materials URL ingest 双端接线 556 防线

日期：2026-10-05。类型：fix(engine+core)。前置：632-634 路由/对偶/差分器面收官；key 复测第六轮不可得（缓行维持）。

## 一、审计：出站数据流全集 vs 556 防线

556 号 SSRF 防线（`web_search.rs::assert_public_egress_host` / `web-search.ts::assertPublicEgressHost`：v4 回环/私网/链路本地/未指定/0.0.0.0/8/广播 + v6 回环/未指定/ULA fc00::/7/链路本地 fe80::/10 + v4-mapped 还原 + 域名 resolve 后全地址判定 + localhost/.local 字面）此前只接在 research_web（`fetch_url`）。本轮核对全部出站点：

| 出站面 | URL 来源 | 判定 |
|---|---|---|
| research_web（Tavily+fetch_url） | LLM 工具参数 | ✓ 556 防线+allowPrivateEgress 豁免（560 号） |
| LLM chat/pipeline 传输 | 用户配置 baseUrl | 设计豁免（LLM 端点本就允许本地推理，llm_endpoint_auth 只影响 key 可选性） |
| 封面/节点图片/play 配图 | 用户配置 cover provider preset | 同上设计豁免 |
| registry bundle 下载 | 固定 URL+minisign pubkey 验签（166 号） | ✓ 无 SSRF 价值 |
| **materials URL ingest（chat 工具 ingest_material + 内部归档）** | **LLM 工具参数/用户输入** | **✗ 双端只校验 scheme，直连任意 URL——绕过 556 防线** |

**缺陷语义**：`readUrlMaterial`/`read_url_material` 对 `http://127.0.0.1:PORT/...`、`http://169.254.169.254/...`（云元数据）等私网 URL 直接 fetch，抓回内容入材料库并被 retrieve_material 召回——LLM 可借"归档网页"之名探测内网并把响应内容带回上下文（数据外带回传通道）。**讽刺点：TS 侧防线函数就在本仓 `web-search.ts`，materials 只是漏接线。**

## 二、修复（双端对称 + 豁免路径）

1. **守卫接线**：TS `ingest.ts readUrlMaterial` + Rust `materials.rs read_url_material` 出站前 `assertPublicEgressHost`。
2. **豁免透传**（与 research_web 同源同形，560 号形态）：复用项目配置 `researchSearch.allowPrivateEgress`（default false）——**从项目配置读而非 LLM 工具参数**（防提示注入让模型自开豁免）：TS `agent-tools.ts` 工具体 `readResearchSearchConfig`；Rust `material_tools.rs tool_ingest_material` 同款。`IngestMaterialInput` 加 `allow_private_egress: bool` 字段；内部归档面（canon-file/style import 均为 file 形态）恒 false。
3. **TS 面试连接线**：`IngestMaterialDeps.allowPrivateEgress` 可选注入，缺省 false。

## 三、红绿可证伪

- **TS**：fetch 计数器探针（`http://127.0.0.1:9/secret` 期望出站前拒且 fetch 零调用）——旧码红（fetch 被调用+无守卫错误）→ 新码绿；豁免正例（`allowPrivateEgress: true` + mock fetch → webpage 归档成功，excerpt 含正文）。
- **Rust**：错误消息断言（旧码错误="Fetch failed: ..."连接层失败，新码="loopback/private address"出站前拒绝）红→绿；豁免正例（`true` → 127.0.0.1:9 discard 端口连接失败="Fetch failed"，证明放行到连接层）。
- **e2e 回归被抓实锤**（修复的第一道真实回归）：`material83_e2e::chat_ingests_url_as_webpage` 用本地 mock 源页——正是豁免存在的正当场景（内网文档源）。fixture 显式写 `{"researchSearch":{"allowPrivateEgress":true}}` 后绿——生产语义即用户允许内网源归档。

## 四、门禁

clippy 双 0；cargo:testgate engine 45 目标 **1906 passed**（+2 Rust 测试）+src-tauri 584 绿；gate:ts 七步全绿（test 步含 TS 4 用例材料面）。bench 裁剪备案：改动面为请求构造前置守卫（非热路径，ingest 每会话低频），且 Rust bench 面不含 materials——留待下轮 bench:gate 例行复验。

## 五、教训

1. **防线建成≠防线全覆盖**：556 号在 research 面建成后，未回头扫其余出站数据流——「安全原语存在的意义是被所有同类场景消费」，新增出站面时接既有原语应是默认动作（本号已把 ingest 面补上）。
2. **豁免的信任边界=配置而非参数**：LLM 工具参数可被提示注入操纵（"把这个 URL 归档，允许内网"），项目配置只有用户/运维能改——豁免通道必须走后者。
3. **修复面测试先行揪出真实回归**（e2e m83）：本地 mock 源正是豁免的正当场景——红绿协议不仅验证修复，还暴露修复的设计边界（豁免语义必须在同一号内定义，否则下游 e2e 大面积红）。

（635号）
