# 929 号 · env 运维键文档对齐轮：README 环境变量表补六键 + 928 尾注勘误

> 912 文档对齐的收尾遗漏面：README「引擎环境变量（常用）」表停在
> 898-925 新增运维键之前——六个用户可调预算/策略键未入表（用户遇到
> 带标签超时/拦截错误后无文档可查调法，与 913 同型）。本轮补齐（两
> 行新增+一行细化）+顺带勘误 928 尾注提交数（59→61）。
> 验证：gate:ts 十步全绿（当日 914-929 九轮连绿）；U+FFFD 净；零代码
> 改动。

## 一、补键明细（898-925 新增运维键全集）

| 键 | 默认 | 来源轮 |
|---|---|---|
| `INKOS_LLM_REQUEST_TIMEOUT_MS` | 300000 | 898（LLM 请求总预算，buffered/流式同族有界；超时 `LLM_REQUEST_TIMEOUT` 不自动重试） |
| `INKOS_NOTIFY_TIMEOUT_MS` | 15000 | 903（通知链预算） |
| `INKOS_COVER_REQUEST_TIMEOUT_MS` | 600000 | 899（封面生成预算） |
| `INKOS_COVER_DOWNLOAD_ORIGIN_POLICY` | default | 900（strict=仅同源+allowlist） |
| `INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS` | — | 900（逗号分隔 origin allowlist） |
| `INKOS_DETECT_HTTP_TIMEOUT_MS` | 30000 | 925（检测链预算） |

- LLM 行为行补 REQUEST_TIMEOUT_MS；新增「网络预算」行（notify/cover/
  detect 三键）；COVER_* 专项行细化 origin 策略双键（双端同键说明）。

## 二、928 尾注勘误

- 928 文档尾注「Fork 推送 59 个提交」为笔误（实为 61：927 收口 60+
  928 本体）。本表下注正确值。

## 三、验证

- gate:ts 十步全绿（当日 914-929 九轮连绿）；U+FFFD 净；零代码改动。

**下一号自 930 起；出口清空回归常备；用户侧待办=Fork 推送 62 个提交（含本提交；勘误：928 文档笔误的 59 应为 61——以本行与 git log 实数为准）。**
