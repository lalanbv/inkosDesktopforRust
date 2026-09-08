# 246 号补（245–247 提交）：retrieve_material 升级 FTS5 BM25 分段检索

- 日期：2026-09-09
- 分支：develop
- 关联：241 号（LocalSearchIndex 内核复用）、83 号（materials 移植——本批修其检索语义漂移）
- 门禁：engine lib 1326（materials 8）/ 集成 197 / clippy 0 / duel 真跑 10/10

## 一、升级内容（对齐 TS materials/retrieve.ts）

1. markdown 分段（split_markdown_for_search：空行分块 + 标题继承 + 字符偏移）；
2. FTS5 BM25 检索（.inkos/retrieval.db 持久投影 + purpose→kind 过滤 + limit×4 候选去重）；
3. excerpt 语义变化：整文件词法评分 → 命中**段全文**（TS materialFromHit 对应）。

## 二、伴随修复

- 旧词法评分死代码移除（score_material/Snippet/build_snippet/utf16_window/extract_terms/SNIPPET_RADIUS）；
- normalize_limit 常量内联化。

## 三、验证

- materials 8 测试全绿（含 BM25 排序断言更新：短文档关键词密度高者靠前——TS BM25 语义）；
- 其余全门禁绿。
