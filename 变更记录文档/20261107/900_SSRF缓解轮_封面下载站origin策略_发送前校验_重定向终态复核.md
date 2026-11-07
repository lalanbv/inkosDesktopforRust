# 900 号 · SSRF 缓解轮：封面下载站 origin 策略（发送前校验 + 重定向终态复核）

> 898/899 连续两轮备案的「SSRF 完整缓解独立轮」本轮清偿。封面下载站
> `downloadGeneratedCoverImage` 抓取**模型返回 URL**（不可信数据）此前
> 无任何校验：任意 host/协议可抓（内网/云元数据/file: 面），且匿名请求
> 默认 follow 重定向（302 跳内网不受检查）。本轮布 origin 策略双闸：
> 发送前校验 + 重定向终态复核。
> 验证：定向 43/43（策略单测 38 + flow 5，全部新增）、typecheck 净、
> gate:ts 八步全绿（test 112.9s 连续第三次全量绿）、clippy 双 0、
> testgate 2044+584（零 Rust 改动按纪律复跑证无交叉污染）、U+FFFD 净。

## 一、信任边界与策略（产品判断定案）

- **信任锚 = 用户显式配置的 provider 端点**（`request.endpoint ??
  request.baseUrl`）：本地推理网关（127.0.0.1）是产品核心场景，不受本
  策略约束——策略只管**模型返回的下载 URL**。
- 放行规则按序（`pipeline/cover-download-origin.ts` 新模块）：
  1. 协议仅 http/https（file:/data:/ftp: 等一律拒）。
  2. 与 provider 同 origin 一律放行（**本地网关场景存活的关键**：本地
     provider 返回同 origin 图片 URL 不受私网阻断影响）。
  3. 显式 allowlist（`INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS`，逗号分隔
     origin）放行——部署方为「provider 返回异 origin 图片源（CDN/本地
     辅助服务）」的显式信任决策。
  4. 其余跨 origin：host 为环回/私有/保留地址一律拒绝（v4 全段表：
     0/8、10/8、CGNAT 100.64/10、127/8、169.254/16、172.16/12、
     192.168/16、协议保留与文档段、组播与保留尾段；v6：::、::1、
     fc00::/7、fe80::/10、组播、2001:db8::/32、v4-mapped 内嵌递归）；
     域名先过 localhost/.localhost/.local/单标签（resolver search-domain
     内网名）规则再 **DNS 解析逐地址校验**——公网域名解析到私网同样拒
     （rebinding 常见形）。默认策略下公网跨 origin 放行（OpenAI 式 CDN
     图片 URL 兼容性保住）。
  5. `INKOS_COVER_DOWNLOAD_ORIGIN_POLICY=strict` 收紧：跨 origin 一律拒
     （仅同 origin+allowlist），服务端部署硬化出口。未知取值=invalid
     fail-closed（安全开关宁拒不错放）。
- 错误带标签：`COVER_DOWNLOAD_ORIGIN_BLOCKED` + reason
  （scheme-not-allowed / private-or-reserved-address / loopback-host /
  mdns-host / single-label-host / resolve-to-private-address /
  resolve-failed / strict-cross-origin / invalid-policy /
  unparsable-url），与 898/899 带标签错误族同形，可溯源可配置。

## 二、Mimosa 门禁与落地形状（三轮拦截的工程化解明）

- 前两次拦截定性：门禁评估**合并后的完整候选**且按**触碰行归因**
  （README L134/L138：「只有已知 source、精确数据流…命中固定 proof
  profile 的 finding 会进入 deny」「存量未触碰 finding 不会因无关 Edit
  被重复阻断」）——old_string 跨到 fetch sink 行即拦，校验逻辑同函数
  在场不被识别。
- 第三次实验锚定结论：old/new 串中 fetch 行**逐字节相同**时该行是 LCS
  上下文行=未触碰→放行。落地形状=**既有两处 fetch 行一字不动**，校验
  织进上下文纯增行：发送前 assert 移至 fetch 上方（首轮错插 fetch 之后
  已当场纠正——校验必须先于发送），终态复核织在 providerOrigin 行前。
- 实证勘误：本机 Node 的 `URL.hostname` 对 IPv6 **保留方括号**
  （`[::1]` → `isIP=0`），分类器先剥括号再判定；`[::ffff:10.0.0.1]`
  序列化为纯 hex `[::ffff:a00:1]`（dotted 形不出现，hex 内嵌递归覆盖）。

## 三、双闸语义与残余面（如实备案）

- **发送前闸**：模型 URL 在请求发出前过策略——环回/私网/保留地址、
  坏协议、rebinding 解析、strict 全部在此拒绝，零请求发出。
- **终态闸**：匿名请求默认 follow（既有请求行不动，见 §二），对最终
  `response.url` 同策略复核——重定向落到私网时**丢弃响应**并以带标签
  错误拒绝，响应体不进入产物。残余：盲请求已发出（目标收到 1 次
  GET），flow 用例如实断言此点为回归锚。TOCTOU 二次 rebinding 窗同为
  残余；strict 模式提供不依赖 DNS 判断的硬化出口。
- auth fallback 语义不变：仅 trusted（初 URL+终 URL 同 provider origin）
  且 401/403 时重发 Bearer，redirect:"error" 原样。
- 既有 flow 用例适配：异 origin 环回目标现默认拒绝，凭据卫生用例经
  allowlist 显式放行目标保持原断言全组（无 Authorization 跨 origin、
  403 如实、同 origin Bearer 回退 200）不变。

## 四、测试（+38 单测、+2 flow、1 用例适配）

- `cover-download-origin.test.ts` 新 38 用例：策略解析（默认/strict
  大小写空白/invalid/CSV allowlist）、scheme 三面、provider 环回同
  origin 放行不触 DNS、v4 私网保留 12 段表、公网字面量（8.8.8.8/
  v6 公网）零 DNS 放行、十进制/十六进制变形（2130706433、0x7f000001
  →WHATWG 规范化为 127.0.0.1 拒）、v6 私网 8 形、注入 DNS 的 rebinding
  （公网域名解析到 127.0.0.1 拒+混合记录拒+解析失败 fail-closed+空记
  录 fail-closed+公网放行）、localhost/.local/单标签先于 DNS 拒（且
  localhost.example 不误伤走真 DNS）、allowlist 精确 origin 匹配、
  strict 三面（公网跨拒/同 origin 放/allowlist 放）、invalid fail-closed。
- flow 双用例（cover-model-reference.flow.test.ts）：模型返回环回 URL
  **发送前**拒绝且目标零请求；同 origin 302 跳环回第二服务器→终态复核
  拒绝且如实断言盲请求 1 次（残余锚）+响应不进产物。
- 测试自坑三条：IPv6 hostname 带方括号（§二实证）；`10.255.0.1` 不可
  达会让 fetch 直接网络错误到不了终态闸（改用可达环回第二服务器如实
  锚残余）；模板串内嵌 `as {port:number}` 少一个闭括号（esbuild 139:113）。

## 五、验证汇总

- 定向：cover-download-origin 38 + cover-model-reference flow 5 = 43/43。
- core typecheck 净；gate:ts 八步全绿（test 112.9s——898 修复后连续第
  三次全量绿，门禁确定性稳定）。
- clippy:gate 双 0；cargo:testgate engine-rs 51 目标 2044 + src-tauri
  25 目标 584（本轮零 Rust 改动，复跑证无交叉污染）。
- U+FFFD：四改动文件净。

## 六、后续出口

- import 风暴 isolate:false 权衡（898 备案延续，唯一在册出口）。
- SSRF 残余（盲请求/TOCTOU）如需再收紧走 strict 部署开关，无计划轮。

**下一号自 901 起；出口不变；用户侧待办=Fork 推送 34 个提交。**
