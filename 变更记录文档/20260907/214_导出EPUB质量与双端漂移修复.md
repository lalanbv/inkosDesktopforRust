# 214 号：导出/EPUB 面质量缺口分析——结构全检 + markdown→html 双端漂移修复

- 日期：2026-09-07
- 分支：develop
- 关联：212 号（beats golden 差分模式——本批复用并第二次立功）、213 号（压测 fixture 复用）
- 推送核验：origin/develop 仍停 6bc65564——**201–213 十三提交均未推送**；本批次后本地领先 14。两项默认值无新答复。

## 一、EPUB 质量全检（60 章真实导出 + 注入测试）

| 检查项 | 结果 |
|---|---|
| mimetype 首位且不压缩（stored） | ✓ |
| META-INF/container.xml / OPF 3.0 结构 | ✓ |
| metadata（identifier/title/language/dcterms:modified） | ✓ |
| manifest/spine/nav 完整（60+1 项对齐） | ✓ |
| 全部 63 个 XML 良构（xmllint） | ✓ |
| 特殊字符转义（注入 `<>&` 章实测：`&lt;`/`&amp;`） | ✓ |
| 段落包裹（空行分 `<p>`） | ✓ |
| 双端参数对齐（title/lang zh-CN|en/同源 markdown→html） | ✓ |
| 500 章 4.5MB 导出 20ms（213 号） | ✓ |

**结论：EPUB 结构面无缺口**（手写 stored-zip 实现符合 EPUB 3 最小规范）。

## 二、缺口：markdown→simple html 无共享守门——差分即抓双端各一个漂移

`markdown_to_simple_html` 双端各有 1 个单测但无共享向量。按 212 号模式建 `export-vectors.json`（10 向量：基础标题/多级滤除/转义/无标题回退/#tag 行/多行取首/tab 分隔/多空格 trim/空标题回退/标题内特殊字符）+ core `golden-export.test.ts` + engine `tests/golden_export_diff.rs` 同源断言。**向量落地即抓到两个真实漂移**：

1. **Node 端缺陷**（TS `^#\s+/m` 的经典陷阱）：`\s` 含换行——章节以「# + 纯空白行」开头时，**下一行正文被误当标题**（实测 title='正文。'）。修复：`^#[ \t]+` 行内空白 + trim 空回退 Untitled。
2. **Rust 端漂移**：`strip_prefix("# ")` 只接受恰一个空格——`#\t标题`（tab 分隔）落到 Untitled 而 Node 提取成功。修复：`strip_prefix('#') + trim_start_matches([' ', '\t'])` + 非空守卫。

修复后双端 10/10 全绿。配套：Node `markdownToSimpleHtml` 导出（原私有）、Rust `markdown_to_simple_html_for_test` pub 包装（集成测试差分入口）。

## 三、验证

- engine：golden_export_diff 1/1（10 向量）、lib **1286**、全部集成套件 ok（7 个 test result: ok）、clippy 零告警、INKOS_DUEL=1 duel **10/10**。
- core：vitest **200 文件 1902 用例**（+10）、typecheck ✓；studio 89 文件 790 ✓（无改动复核）。
- EPUB 检查为一次性验证（结构由实现与既有测试守门；xmllint 全检证据在变更记录）。

## 四、结论

导出/EPUB 面质量闭环：结构规范 ✓、转义 ✓、性能 ✓（213 号）、**markdown→html 语义双端钉死**（共享向量，两个漂移已修）。golden 差分模式两次落地两次立功——后续新增双端纯函数域应默认带向量守门。
