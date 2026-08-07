# 变更记录：registry http_fetch HTTPS-only 强制

## 日期
2026-08-07

## 变更类型
feat(security): registry 下载强制 https:// scheme

## 问题
`http_fetch` 接受 `http://` URL，明文传输暴露安装的插件列表（流量指纹）
和用户隐私。虽然 bundle 有 Ed25519 完整性签名，但 MitM 可观察元数据。

## 修复
- `http_fetch` 最前置：非 `https://` 开头 → `bail!`
- 4 个 SSRF 测试 URL 从 `http://` 更新为 `https://`（SSRF 守卫仍在 IP 层生效）
- 新增 `test_http_fetch_rejects_http_scheme` 测试

## 累计测试数
519 passed, 0 failed（全量）
