# 924 号 · 900 族洞 Rust 镜像修复：cover_origin 策略模块 + Bearer 泄露面收口（双端同键同语义）

> 修洞按族勘察（900 的 Rust 镜像核查）：`engine-rs/src/llm/cover.rs`
> 头注自证「b64_json 或 **url → 下载**」——Rust 引擎侧同款模型 URL 下
> 载面存在且 900 只修了 TS=**族洞实锤**，且 Rust 形态比 TS 旧态更宽：
> 零校验+reqwest 默认 follow 重定向+**401/403 无条件带 Bearer 重试**
> （任意 URL 的 401/403 都把 api_key 发过去=凭据泄露面）。
> 修复=cover_origin.rs 策略模块（900 镜像）+cover.rs 双闸集成。零 TS
> 改动；双端 env 同键同语义（INKOS_COVER_DOWNLOAD_ORIGIN_POLICY/
> INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS，部署一处配置两端生效）。
> 验证：cover_origin 7/7+cover 族回归绿+testgate **2051**（+7）+584、
> clippy 双 0、audit:rust 双 0、gate:ts 十步全绿、U+FFFD 净。

## 一、族洞全景（与 TS 900 旧态对照）

| 维度 | TS 900 修复前 | Rust 修复前（本轮起点） |
|---|---|---|
| origin/scheme 校验 | 零 | 零 |
| 重定向 | 默认 follow | reqwest 默认 follow |
| 401/403 Bearer | 有（无 origin 门控） | **无条件带**（任意 URL 401/403 都发 key） |

## 二、修复

### cover_origin.rs 新模块（策略+校验，与 TS 版同表同语义）

- 策略解析（mode default/strict/invalid fail-closed+allowlist CSV）；
- 分类表：v4 全段（0/8、10/8、CGNAT 100.64/10、环回、169.254、
  172.16/12、192.168/16、协议保留与文档段、组播与保留尾段）+v6
  （::、::1、fc00::/7、fe80::/10、组播、2001:db8::/32、v4-mapped 内嵌
  递归）；域名 localhost/.localhost/.local/单标签；DNS 解析逐地址校验
  （spawn_blocking+ToSocketAddrs，任何私网=拒绝防 rebinding，解析失败
  fail-closed）。
- 错误形态：String 带 `COVER_DOWNLOAD_ORIGIN_BLOCKED` 标签+reason
  （与 TS 版同标签同 reason 集），宿主错误链直接透传。

### cover.rs 双闸集成

- **发送前闸**：download_generated_cover_image 加 provider_base_url 参，
  策略校验先于任何请求（finish_images_response 三 API 共用尾面同步加
  参——images/responses/edits 全走下载尾面）。
- **终态闸**：reqwest 默认 follow（请求行不动），对 response.url()
  （follow 后最终 URL）同策略复核——重定向落私网时响应不进产物（盲
  请求残余与 TS 同备案）。
- **Bearer 泄露面收口（顺带族内修复）**：401/403 重试仅 provider 同源
  才带 key（原形态任意 URL 401/403 都发 api_key）。

### 测试（cover_origin 7 用例+既有回归适配）

- 新 7 用例：策略解析三态+allowlist CSV/scheme 三面与 unparsable/
  provider 环回网关信任/v4+v6 私网保留 15 形字面量表（零网络）/公网
  字面量+域名规则（loopback/mdns/single-label）/allowlist+strict+
  invalid 三态强制/带标签错误形态。
- 既有回归适配：generate_images_api_b64_and_url_download 的下载服务
  器与 provider 异 origin=「provider 返回异 origin CDN」场景，测试内
  allowlist env 显式放行（900 产品语义镜像）+测后 remove。

## 三、测试自坑四条

- url crate `host_str()` 对 IPv6 **带方括号**（TS URL.hostname 同款实
  证镜像）——分类器剥 [] 再 parse。
- `::1` 的 v6 判定：segments[7]=1 使「全零断言」失效——环回判法=
  前 7 段全零+尾段 ≤1。
- finish_images_response 共用尾面函数的全局替换命中测试内联调用——
  签名同步+调用点（含测试）三处对齐。
- rustdoc 编号列表 lazy continuation（895 同款）——头注列举改顿号
  连排。

## 四、验证汇总

- cover_origin 7/7+cover 族回归绿；cargo:testgate engine-rs **2051**
  （+7）/src-tauri 584；clippy:gate 双 0（首跑 doc-list 告警已修）；
  audit:rust 双 0；gate:ts 十步全绿；U+FFFD 净。
- TS 面零改动（900 形态不动）；双端语义互为镜像（同 env 键/同规则
  序/同标签）。

## 五、后续出口

- 900 族洞双端闭环（TS+Rust）。出口清空回归常备。

**下一号自 925 起；出口清空回归常备；用户侧待办=Fork 推送 57 个提交。**
