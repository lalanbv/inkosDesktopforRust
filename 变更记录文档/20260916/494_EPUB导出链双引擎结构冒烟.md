# 494 号：EPUB 导出链双引擎结构冒烟——外部契约等价，内部打包各异

日期：2026-09-16　分支：develop　基线：c53f407b（493 号）

## 选题与实现

EPUB 导出是 214 号验证过的核心用户面，但从未在**双引擎对照**下活体验证。本号新增 `scripts/export-epub-smoke.mjs`（与 node-fallback-smoke 同族编排）：临时根 + fixture + mock → 双引擎 `POST /books/:id/export-save {format:"epub"}` → python 标准库结构校验（214 号检查项子集）：

- mimetype 为首条目且 ZIP_STORED、值为 `application/epub+zip`；
- META-INF/container.xml 在场且可解析、rootfile 指向 OPF；
- OPF dc:title 非空、spine ≥ 2；
- 全部 xhtml/opf XML 良构。

## 活体结果

**node 腿 4/4 + rust 腿 4/4，exit 0**——双端各落盘合法 EPUB（chapters=2）：

- 共同外部契约：export-save 返回 `{ok,path,format:"epub",chapters}` 语义一致；EPUB 三大结构（mimetype/container/OPF）与 XML 良构全过——**读取器面等价**；
- 内部打包实现各异（符合 486 号"锁外部契约、放内部实现"）：node 走 epub-gen-memory（style.css + toc.ncx + toc.xhtml + 数字命名章节），rust 走原生 epub crate（nav.xhtml + chapterN 命名、无 ncx）——两者均为合法 EPUB 3 形态。

## 门禁

- 套件：双腿 8 断言全绿 exit 0（腿间 1s 端口释放间隔——SIGTERM 后立即重 bind 会 EADDRINUSE）；
- 全量 `pnpm -r test`：3237 全绿（本号零产品代码改动）；
- cargo 链接仍被 Xcode 许可阻断（481 号欠账 + 489 号 2 新单测持续排队）。

## 改动

- 新增 `scripts/export-epub-smoke.mjs`（双引擎 EPUB 导出结构冒烟）。

## 后续

- Xcode 许可解除后：481 号全量 cargo test、489 号 2 新单测、duel、bench:gate、skills/genres 移植评估；
- 三库种子 canonical 化待产品拍板；npm 接受风险余 2 条上游钉死项滚动跟踪。
