# 267 号：HEAD 全栈生产形态端点冒烟扫（255–266 号十二批提交后的整合回归）

- 日期：2026-09-10
- 分支：develop
- 关联：251 号（冒烟先例）、255–266 号（被整合验证的十二批）
- 推送核验：fetch 实测 origin/develop 仍停 6bc65564（200 号），用户尚未推送；本地领先 **72** 提交。两项默认值无新答复。

## 一、端点冒烟扫（生产形态 bin + fixture 项目，4790 端口）

| 端点/守卫 | 结果 |
|---|---|
| GET /health、/books、/logs、/services、/skills | ✓ 全 200 |
| GET /interactive-films | ✓ 200 + 正确 film 列表（`/projects` 列表 404 系冒烟路径笔误，列表域实为 `/interactive-films`，TS 同构） |
| GET story-graph / validation / analysis | ✓ 全 200 |
| GET export ink / html / json | ✓ 全 200 |
| 穿越守卫 `books/..%2F..%2Fetc` | ✓ 400 拒绝（201 号 segment_guard 有效） |
| 非法 sessionKind | ✓ 400 INVALID_SESSION_KIND |
| authoring 会话无 bookId 建会话 | ✓ 200（预期：门控在 /agent 运行面为 BOOK_ID_REQUIRED——256 号已验；建会话面与 TS 同构不设限） |

## 二、负载备案

本机 loadavg 13/18 核（72%/核）远超 bench:gate 20% 阈值——性能门禁全量真跑待静默窗口（拒绝行为已在 265 号实测正确），基线维持 259 号全绿结论。

## 三、结论

255–266 号十二批改动后的 HEAD 在生产形态下端点面健康，安全守卫有效。零代码改动批。
