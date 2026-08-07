# 变更记录：extract_host 反斜杠 authority 解析器分歧绕过

## 日期
2026-08-07

## 变更类型
fix(security): extract_host 将 `\` 计入 authority 终止符（对齐 WHATWG）

## 问题（解析器分歧 → 白名单 + SSRF 双层同时绕过）

`extract_host` 的 authority 终止符集为 `['/', '?', '#']`，不含反斜杠。
但 WHATWG URL 规范对 http/https 这类 **special scheme** 规定 `\` 等同 `/`，
ureq 2.12 依赖的 `url` 2.5 遵循该规范。两个解析器由此分歧：

```
URL: http://127.0.0.1\x@allowed.example.com/p

本函数（修复前）：`\` 不终止 authority
  → authority = "127.0.0.1\x@allowed.example.com"
  → rsplit_once('@') 剥 userinfo → host = "allowed.example.com"
  → check_network_domain 放行（白名单内）
  → is_internal_ip("allowed.example.com") = false（看不到 IP）

ureq / url crate：`\` 终止 authority
  → 实际连接 127.0.0.1
```

已用 `url` 2.5 探针实测确认三种形式的 host 归属：
`evil.com\x@allowed…` → `evil.com`；`127.0.0.1\x@allowed…` → `127.0.0.1`。

危害：域名白名单与 SSRF 内网字面量拦截**同时失效**。插件仅声明
`network:allowed.example.com`，即可访问 `127.0.0.1` 与云元数据
`169.254.169.254`；ssrf_resolve 的 IP pinning 也无从介入
（它对拿到的 host 做过滤，而此处 host 已被误判为公网域名）。

## 修复
`find(['/', '?', '#'])` → `find(['/', '\\', '?', '#'])`，使 authority
边界与 ureq 实际连接目标一致。分歧消除后，`\` 之前的真实主机进入
白名单比对与 `is_internal_ip` 判定。

## 测试（+1 测试 +3 断言）
- `test_extract_host` 扩展 3 条断言：`127.0.0.1\x@allowed…` → `127.0.0.1`、
  `evil.com\@allowed…` → `evil.com`、`evil.com\.allowed…` → `evil.com`
- `test_backslash_authority_does_not_bypass_network_policy`：断言**端到端语义**
  （4 种绕过形式全部 `!check_network_domain`，内网形式 `is_internal_ip`=true，
  正常 URL 与子域仍放行）。刻意不断言 extract_host 返回值——
  将来若改用 url crate 解析，这两条策略约束仍须成立。

## 验证
- `cargo clippy --all-targets -- -D warnings`：clean
- `cargo test`：**522 passed / 0 failed**（+1 净增，零回归）
