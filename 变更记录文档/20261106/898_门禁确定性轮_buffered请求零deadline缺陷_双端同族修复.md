# 898 号 · 门禁确定性轮：gate:ts「饱和 flake」根因定案=buffered LLM 请求零 deadline 缺陷（双端同族修复）

> 897 号备案的 gate:ts test 步「饱和型 flake」经本轮根因定案：**不是测试预算
> 问题，是产品级传输缺陷**——`chatCompletion` buffered（stream:false）路径
> 完全无 deadline，fetch 以 signal-less init 发出，端点停摆即永久挂死且不可
> 取消，只能靠 undici 自身 300s headersTimeout 逐次兜底。三跑受害者签名统
> 一（测试体尸挂 ~912s 至套件尾收尸，**3/3 必现**），双端同族修复（Rust 非
> 流式无看门狗是同一洞的忠实镜像）。修复后健康全量 core=55.44s（原
> 966.83s，差值=尸挂本体），gate:ts 八步全绿恢复确定性红绿。
> 验证：TS 定向 11/11、Rust 定向 27/27、clippy 双 0、testgate 2043(+3)+584、
> gate:ts 八步全绿（test 111.3s）、U+FFFD 净。

## 一、证据链与定性（897 定案的修正）

### 三跑失败面重读（尸挂签名统一，非 20/30s 快速超时）

- run1/run2：`short-source.flow` 失败——但文件计时 **918130ms**（run2 日志
  实证），非 20s；run3：longform-harness resume **912592ms** +
  short-revision finalize **911834ms**。三跑受害者的测试体都在 **~912-918s**
  才 settle；20s/30s testTimeout 只是提前盖「失败」章，测试体继续挂到套件
  尾被收尸（run3 longform afterEach `ENOTEMPTY` 实证：测试体超时后仍在写
  临时目录）。受害者在漂移（short-source ×2 → longform+finalize），但
  **~912s 尸挂时长是不变量**。
- 配对复现未中：受害者两两配对（2 文件=maxWorkers:2 同屏）21/21 绿 16s；
  34 个 flow 文件全组 2-workers 33.79s 绿——挂起需要全量套件语境（import
  风暴：run2 画像 import 求值 929.49s=48% 墙钟，538 模块 ×1137 次求值）。

### 算术闭合

undici 全局 dispatcher 默认 `headersTimeout=300_000ms`；受害者 ~912s ≈
**3 次 buffered LLM 请求 × 300s + 退避/实工 ~12s**（管线 agent 多轮逐次
停摆，或重试环放大——`withTransientLLMRetry` 的传输态分类喂的是 undici
超时后的错误文本）。每轮挂 300s、无标签、不可取消、套件墙钟被撑到
~16 分钟（run3 tests=99%）。

### 根因（静态实锤）

`packages/core/src/llm/provider.ts` chatCompletion：

```ts
const deadline = client.stream ? createStreamActivityDeadline(...) : undefined;
```

流式有 first-event/idle 双窗，**buffered 分支 `undefined`**——`deadline?.signal
?? signal` 退化为 caller signal（管线调用通常为空）→ fetch 无 signal。而
`createRequestDeadline`（300s 默认 + INKOS_LLM_REQUEST_TIMEOUT_MS env）
本就是为此写的（doc comment "Buffered responses have a total request
deadline"），但只在 pi-stream 路径接线，chatCompletion 漏接。
**Rust 同族镜像实证**：`streaming_client.rs` 非流式分支注释自认「非流式无
看门狗（TS client.stream 同款门）」——`req.send()`/`resp.text()` 零窗。

### 残留未定案（诚实备案）

停摆的**触发器**（全量语境下本地 mock 服务器为何偶发不响应）未钉死——
候选：keep-alive 半关竞争（服务器侧 `for await (chunk of req)` 因 socket
半关提前收束 → `JSON.parse` 空体抛错 → handler 无响应 → 客户端挂）或
import 风暴期 worker 事件循环饥饿。short-revision finalize（纯 mock 无
HTTP）的独立挂点同样未定案。修复不消灭触发器，但把全族上限压到 ≤300s
且带标签（LLM_REQUEST_TIMEOUT）——下次触发直接给出诊断面（哪条请求、
哪个端点、多久），尸挂形态终结。

## 二、修复（双端同族）

### TS（packages/core/src/llm/provider.ts）

1. buffered 分支接 `createRequestDeadline(signal)`：总窗 300s 默认、
   `INKOS_LLM_REQUEST_TIMEOUT_MS` env 可调（readPositiveTimeout 同序）、
   `AbortSignal.any` 组合 caller signal——**不可取消挂死 → 可取消**。
2. `createRequestDeadline` 返回面增 no-op `activity()`（缓冲路径无流活动
   时钟，统一两族 deadline 的调用面）。
3. `LLMRequestTimeoutError` 原生 `code:"LLM_REQUEST_TIMEOUT"` 且
   `isRetryableLLMError` 显式 false——**不进重试环**（300s 挂 ×1 而非 ×3）；
   `wrapLLMError` fallthrough 保留原实例。

### TS 测试（llm-retry-behavior.test.ts +2）

- 停摆 buffered 请求在 env 窗（10ms）到期报 `LLM_REQUEST_TIMEOUT`，
  恰 1 次 fetch（不可重试），**回归锚=fetch init.signal 必为 AbortSignal**
  （修复前 undefined）。
- caller abort 组合进 buffered deadline：在途请求即刻中断（修复前 abort
  够不着 signal-less fetch）。

### Rust（engine-rs/src/llm/streaming_client.rs，TS 逐字对齐）

1. `StreamDeadlineSpec` 增 `request_ms`（INTERACTIVE/PIPELINE 双面
   300_000，对齐 TS createRequestDeadline 默认）+ `ENV_REQUEST_TIMEOUT
   ="INKOS_LLM_REQUEST_TIMEOUT_MS"`（env > 面默认，同名同序逐字）。
2. `StreamError::RequestTimeout{timeout_ms}` 变体 + Display 逐字
   "LLM request did not complete within {timeout_ms}ms"。
3. **chat 与 responses 双传输**非流式分支：总窗覆盖 send/错误体/响应体
   三面（`timeout_at` 包裹；BadStatus 错误体读取一并入窗），到期报
   RequestTimeout。流式路径看门狗原样（130 号语义不动）。
4. 测试 +3：停摆本地端点（accept 后持有不读不写不关）50ms 总窗到期报
   RequestTimeout 且耗时 <5s；Display TS 逐字；env 解析序（合法/非法回
   退）。既有 6 处 spec 字面量 + 4 处 resolve_deadline 调用同步更新。
   `agent_router` 经 `StreamDeadlineSpec::resolve` 装配——env 全链可达。

## 三、语义与差异备案

- 默认 300s 双端一致；env 名双端逐字同名。TS 侧 deadline 无 per-call
  override 参数（env+default 两级）=pi-stream 先例原样；Rust resolve 亦
  不增 override 参数（接口面零变动）。
- caller abort：TS 修复后组合可达（AbortSignal.any）；Rust
  ChatCompletionParams 本无 caller abort 通道——差异备案（既有形态，非
  本轮引入）。
- Rust 流式路径 BadStatus 错误体读取未套窗（流面 residual，TS 流式侧由
  deadline.signal 覆盖）——备案不改，属下一族扫候选。
- 流测试预算（20s 显式/30s 全局）**不动**：根修后单请求上限 300s 带标签，
  通胀预算=洗失败（846 纪律），不做。

## 四、验证汇总

- TS 定向：llm-retry-behavior+llm-retry 11/11（含新增 2 用例）。
- Rust 定向：streaming_client 27/27（含新增 3 用例）。
- clippy:gate：engine-rs + src-tauri 双 0。
- cargo:testgate：engine-rs 51 目标 **2043 passed（+3=新增）**、src-tauri
  25 目标 584 passed。
- gate:ts 八步全绿（exit 0）：build 15.8s / typecheck 14.4s / **test
  111.3s** / audit:npm 2.6s / rust-bin 0.2s / node-fallback-smoke 35.8s /
  engine-contract-diff 36.2s / export-epub-smoke 35.4s。
- **健康基线实测（修复后手动复跑 `pnpm -r test`，1:50.59s 全绿）**：
  core 183 文件 **1016 用例 55.44s**（tests 88%/import 12%；+2=本轮新增
  deadline 用例）、studio 141 文件 755 用例 45.79s、cli 23 文件 81 用例
  3.22s。**core 健康时长 55.44s vs 此前三跑 966.83/972.97s——差值
  ~912s=尸挂本体**（912+55≈967 精确吻合），run2「import 929s/48%」画像
  同为挂死 worker 扭曲的统计。此前 gate:ts 慢形态（typecheck 970s/
  contract-diff 1010s/epub 943s）多为冷缓存+尸挂叠加，非步骤本体成本。
- **定性再修正（对 897 的二次勘误）**：修复前三跑 **3/3 必现尸挂**
  （受害者漂移但必挂），非「概率性 flake」——895/896 提交信息所载
  「gate:ts 八步绿」与本证据链矛盾（flake 形态下绿跑需幸存者运气），
  以本证据链为准。
- U+FFFD：五改动文件净。

## 五、后续出口

- 下次全量若 test 步仍红：新形态必为 ≤300s 带标签 LLM_REQUEST_TIMEOUT
  （或非 LLM 挂点暴露）——触发器定案的直接证据，届时按标签溯源。
- import 风暴（929s/48% 墙钟）：isolate:false 是大杠杆但有共享模块状态
  风险，另轮评估。
- Rust 流面 BadStatus 错误体套窗：族扫候选。
- **899 族扫候选（本轮普查登记，同类不同面）**：notify 四通道
  （feishu/webhook/telegram/wechat-work）与 runner 封面三站
  （short-fiction-runner.ts 1326/1453/1555）均为裸 fetch 无 signal——
  同属「无界网络请求」类，但预算语义各异（webhook 应短窗 ~10-30s、
  封面生成分分钟），须按面定预算再修，不宜本轮硬套 300s。

**下一号自 899 起；出口不变；用户侧待办=Fork 推送 32 个提交。**
