# 变更记录：registry http_fetch SSRF 防护 + 禁重定向

## 日期
2026-08-07

## 变更类型
feat(security): registry 下载路径 SSRF 防护

## 问题
`http_fetch`（registry 索引 + bundle 下载）直接 `client.get(url)` 无任何 SSRF 校验。
被篡改/入侵的注册表可将 `download_url` 指向 `169.254.169.254`（云元数据）或内网服务。
reqwest client 也允许重定向，合法域名可被跳转到内网（开放重定向 SSRF）。

## 修复
1. `http_fetch` 前置：`extract_host + is_internal_ip` → 内网/保留 IP 字面量即 bail
2. `registry_source_and_client` 中 reqwest Client：`.redirect(Policy::none())`

## 测试
新增 4 个 SSRF 测试：127.0.0.1 / 192.168.x.x / 169.254.169.254 / [::1]
全部在连接前被拒绝（错误含 "SSRF" 或 "内网"）

## 累计测试数
516 passed, 0 failed（全量）
