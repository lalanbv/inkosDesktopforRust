# 165 · Rust 引擎 GET /project 读侧对齐 Node——修复无 llm 键项目启动 500

## 一、问题（用户报告）

桌面端（164 号切 Rust 引擎后端后）启动即报：

```
无法加载项目配置 / Failed to load project config
Failed to load inkos.json: Invalid input: expected object, received missing
请检查项目根目录下的 inkos.json 是否存在且为合法 JSON，然后重试。
```

## 二、根因

**Rust 引擎 `GET /api/v1/project` 的读取闸门比 Node 严格**（读侧契约差，
非桌面壳问题）：

- Node（`server.ts` L3987 → `resolveEffectiveLLMConfig`）：
  `const llm = { ...((config.llm ?? {})) }` —— **`llm` 键缺失 → 空对象**；
  GET 路径 `requireApiKey: false` → `fillNoopLLMDefaults` 填充
  provider=openai / baseUrl=https://example.invalid/v1 / model=noop-model
  → schema 终验通过 → **200**。
- Rust（`project_config_routes.rs` get_project）：
  `obj.get("llm").and_then(as_object) else → 500`，报错文案复刻了 zod 的
  缺字段措辞——但 Node 的 zod 报错只发生在**填充后**的终验，读取层永远先补
  `llm`，该闸门是移植时的语义误植。

实证：`test-project/inkos.json`（无 `llm` 键，真实遗留形状）双端对跑——
Node 200 noop 填充 / Rust 500（修复前）。164 号前桌面跑 Node 故从未暴露；
duel/契约夹具均带合法 `llm`，未覆盖此形状。

## 三、修复（读侧三态逐字对齐 Node `readProjectConfig`）

| 输入形状 | 修复前（Rust） | 修复后（Rust） | Node（对照） |
| --- | --- | --- | --- |
| `llm` 键缺失 / null / 非对象 | 500 `received missing` | **200** noop 填充 | 200（`?? {}` + 展开语义） |
| `llm` 字段空串（baseUrl/model=""） | 200 noop（既有） | 200 noop（不变） | 200（`fillNoopLLMDefaults` 空串替换） |
| 文件缺失 | 500 `Unexpected token` | 500 `inkos.json not found in {root}.\nMake sure…`（Node 指引文案） | 同左 |
| JSON 语法错误 | 500 `Unexpected token` | 500 `inkos.json in {root} is not valid JSON. Check the file for syntax errors.` | 同左 |
| 合法 JSON 根非对象（如 `42`） | 500 `Unexpected token` | 500 name 校验（落到 zod 终验面对应位） | 500（zod name） |

「优化」面：缺文件/坏 JSON 的报错从无信息量的 `Unexpected token` 换成 Node 的
可操作指引（含项目根路径与 `inkos init` 提示）；此前用户按旧文案「检查 JSON
合法性」排查会走偏——文件合法但缺 `llm` 键才是真因。

## 四、验证（全部真跑）

| 面 | 结果 |
| --- | --- |
| 新增回归 `get_project_tests`（6 用例：无 llm 键/空串/null/缺文件/坏 JSON/非对象根） | **6/6 过**（经完整 router oneshot，非直调 handler） |
| engine-rs 全量 `cargo test --lib` | **1208 过 / 0 失败**（1202 既有 + 6 新增） |
| 端点契约 `e2e_write_next_contract` config 模块 | **14/14 过**（合法 llm 夹具零回归） |
| `cargo clippy --lib --tests -D warnings` | 零警告 |
| 真实 server 冒烟（release 重建 + test-project 实形状） | `GET /api/v1/project` → **200** `{"model":"noop-model","provider":"openai","baseUrl":"https://example.invalid/v1",…}`（与 Node 同形）；启动面横扫 services/daemon/books/interactive-films 全 200 |
| 桌壳资源刷新 | `desktop-package-rust-engine.sh` 重跑（sha256 更新，.app 资源同源） |

## 五、用户侧生效方式

重启 inkosDesktop 即可（dev 二进制解析 `engine-rs/target/release`、打包资源
`engine-rust/` 均已刷新为修复版）。项目无需改动 inkos.json——无 `llm` 键/
空串形状恢复 Node 时代的 noop 默认展示，后续在 Studio 服务设置里配置真实
服务即写入合法 `llm`。

## 六、关联提交

- 本轮：fix(engine-rs): GET /project 读侧对齐 Node——无 llm 键 noop 填充、缺文件/坏 JSON 指引文案（165 号）
