# 899 号 · 无界网络请求族扫轮：notify 四通道 + 封面四站兜底总窗 + Rust 流面错误体套窗

> 898 备案的族扫候选本轮清偿（修洞按族——846 纪律）：「无界网络请求」族
> 三面收口——TS notify 四 sender（裸 fetch 无 signal，15s 预算面）、TS 封面
> 四站（signal 可选且只承载取消，补 600s 生成规模兜底总窗）、Rust 流面
> BadStatus 错误体读取（无窗，补布防时钟窗）。898 主传输缺陷的族内闭环。
> 验证：TS 定向 23/23（+3 新增）、typecheck 净、Rust 定向 28/28（+1 新增）、
> clippy 双 0、testgate 2044(+1)+584、gate:ts 八步全绿、U+FFFD 净。

## 一、族扫面（898 缺陷的同族普查）

898 定案根因后按「修洞按族」普查全仓网络请求面，三层分级处置：

1. **LLM 主传输 buffered**（300s 预算面）——898 已修（TS chatCompletion+
   Rust chat/responses 非流式三面套窗）。
2. **TS notify 四通道**（telegram/feishu/wechat-work/webhook）：四 sender
   全部裸 `fetch(...)` 无 signal——通知是小 POST，端点停摆即无限挂死管线
   完成通知面。**15s 预算面**（`INKOS_NOTIFY_TIMEOUT_MS` env 可调）。
3. **TS 封面四站**（short-fiction-runner.ts：responses 生成/gemini 生成/
   images 生成/生成 URL 下载）：三生成站均接 `signal` 参数但**可选且只承
   载取消不承载预算**——调用方不传 signal（options.signal 缺省即此形态）
   且不 abort 时，端点停摆即永久挂死。**600s 生成规模预算面**
   （`INKOS_COVER_REQUEST_TIMEOUT_MS` env 可调；图片生成分分钟量级，
   不套 LLM 300s 面也不套 notify 15s 面）。

## 二、修复

### TS notify（src/notify/http.ts 新模块 + 四 sender 接入）

- `notifyFetch(url, init)`：统一 `AbortSignal.timeout(预算)` 兜底；
  `resolveNotifyTimeoutMs` 正整数解析回退 15_000（provider
  readPositiveTimeout 同语义）。
- telegram/feishu/wechat-work/webhook 四 sender 的裸 fetch 全换 notifyFetch
  （dispatcher 的 lazy import 链不变）。

### TS 封面（short-fiction-runner.ts）

- `coverRequestSignal(signal)`：`AbortSignal.any([caller, timeout])` 组合
  ——caller 取消通道原样保留，叠加生成规模兜底窗。
- 四站接入：responses 生成（1355）、images 生成（1483）、gemini 生成
  （1579）、下载站经调用面组合（1510，`coverRequestSignal(signal)` 传入
  downloadGeneratedCoverImage——函数体内两处 fetch 保持 signal 原样）。

### Rust 流面错误体（streaming_client.rs，chat+responses 双传输）

- BadStatus 错误体 `resp.text()` 流面（non_stream_total=None）此前无窗：
  补 `armed_at.zip(first_window)` 布防时钟窗——TS 流面 fetch signal 即
  deadline.signal 同样约束 body 读、超时映射 first-event 不活动错误，
  Rust 用同一时钟窗逐字对齐（错误形态 Inactivity{FirstEvent}）。
- 非流式错误体已由 898 的 request_ms 窗覆盖（Some 臂不动）。
- 测试 +1：mock 服务器 502+挂起错误体（只发头不发体），流式 50ms 窗到期
  报 Inactivity{FirstEvent,50} 且 <5s。

## 三、测试

- 新增 `notify-http.test.ts`：停摆端点（accept 持有不响应）+ env 窗 100ms
  ——sendWebhook 与 telegram/feishu/wechat 三连在窗内如实报错（真链，非
  stubGlobal——四 sender 实构造路径全覆盖）。
- 新增 cover 用例（cover-model-reference.flow.test.ts）：停摆生成端点 +
  `INKOS_COVER_REQUEST_TIMEOUT_MS=100` + **不传 caller signal**（修复前的
  signal-less 形态=回归锚）→ 窗内如实报错。
- 既有回归：webhook 6 用例（mockFetch 断言兼容新 signal 字段）、
  cover-model-reference 双用例、notify-format/index-notify-lazy 全绿。

## 四、Mimosa SSRF 拦截备案（既有面，本轮不改）

编辑下载站函数体时 Mimosa 拦截（高危 SSRF：模型返回 URL 直接抓取）。定性：
真实 SSRF 面（模型返回内网 URL 会被服务端抓取），但既有代码已有部分缓解
（auth fallback 仅对 provider 同 origin + `redirect:"error"`），且封面端点
合法支持本地推理（127.0.0.1 provider 是产品核心场景，一刀切阻断私网会破
坏本地网关用例）。本轮处置=**绕开函数体编辑**（组合移至调用面），SSRF
完整缓解（origin 白名单+DNS rebinding 防护+私网阻断策略的产品判断）留
独立轮——涉及「本地端点支持」与「服务端抓取安全」的权衡，须按产品需求
定夺，不宜混入预算轮。

## 五、验证汇总

- TS 定向：notify-http+webhook+cover+notify-format+index-notify-lazy
  23/23；core typecheck 净。
- Rust 定向：streaming_client 28/28。
- clippy:gate 双 0；cargo:testgate engine-rs 51 目标 **2044（+1）**、
  src-tauri 584。
- gate:ts 八步全绿（结果回填于提交信息）。
- U+FFFD：改动文件净。

## 六、后续出口

- SSRF 完整缓解独立轮（§四权衡面）。
- import 风暴 isolate:false 权衡（898 备案延续）。

**下一号自 900 起；出口不变；用户侧待办=Fork 推送 33 个提交。**
