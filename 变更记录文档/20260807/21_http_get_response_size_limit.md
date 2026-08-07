# 变更记录：http_get 响应体大小守卫

## 日期
2026-08-07

## 变更类型
feat(security): http_get 响应体 8MiB 守卫（防 OOM）

## 问题
`ureq::Response::into_string()` 将全响应体读入内存，无大小限制。
恶意服务器可返回 GB 级响应导致宿主进程 OOM。

## 修复
`const MAX_HTTP_RESPONSE_BYTES: u64 = 8 * 1024 * 1024`（8 MiB）；
改用 `resp.into_reader().take(MAX_HTTP_RESPONSE_BYTES).read_to_string()`——
超过 8 MiB 后 take 截断，超截断读取会安静截止（不报错）。

## 注意
take 截断后 read_to_string 不返回错误（仅读前 N 字节），不超限时完整返回。
对 HTTP API（JSON/文本）8 MiB 远超实际需要；大文件下载应通过进程插件处理。

## 累计测试数
519 passed, 0 failed（全量）
