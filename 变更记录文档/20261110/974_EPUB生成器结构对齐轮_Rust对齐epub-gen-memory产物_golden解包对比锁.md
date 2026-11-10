# 974 号 · EPUB 生成器结构对齐轮：Rust build_epub 对齐 epub-gen-memory@1.1.2 产物（golden 解包对比锁）

> 971 号备案「epub 生成器字节级对齐不做（容器兼容已验）」的本轮清偿：以
> `epub-gen-memory@1.1.2` 同输入产物固化为 fixture，解包逐文件对比 Rust build_epub，
> 结构面（文件集/容器/OPF 骨架/NCX/章节头模板/转义策略）全部对齐。
> 验证：Rust golden golden_tests 1/1（新）+translation lib 29/29（+1）+translations70
> 11/11+testgate engine-rs 2109（+1）/src-tauri 584+audit:rust 双 0（自检活体）+clippy
> 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、TS 参考产物勘察（gen-ts-epub.mts 同输入固化 fixture）

- `EPub({title, lang}, chapters)`（title/lang 经 fork escapeHtml）→ 产物九文件：
  `mimetype`（STORED）/`OEBPS/style.css`/章节 `0_.xhtml`、`1_.xhtml`（`${index}_.xhtml`）/
  `META-INF/container.xml`/`OEBPS/content.opf`/`OEBPS/toc.ncx`/`OEBPS/toc.xhtml`。
- 结构要点（解包逐字节勘察）：**EPUB 3**（OPF version="3.0"）+ `toc.xhtml` nav
  （properties="nav"，EPUB 2 的 NCX 同时保留并 `spine toc="ncx"`）+ `<guide>` 目录
  reference；OPF metadata 含 identifier-type 22/dcterms 反射五连/anonymous creator·
  publisher·rights/`dcterms:modified`（秒级 ISO）+`dc:date`（毫秒级 ISO）/generator=
  epub-gen/ibooks:specified-fonts；container.xml 单行紧凑形无尾换行；所有文本文件
  无尾换行。

## 二、Rust 对齐（build_epub 重写 + escape 拆分）

- build_epub 按 TS 模板逐字节重写（模板串自参考产物反推固化）：EPUB 3 OPF 骨架/
  metadata 全块/manifest（ncx+toc+css+content_N_item_N）/spine（toc 前置）+guide/
  toc.xhtml nav（li class=table-of-content，a 文本=章节标题原文+空白行）/toc.ncx
  （head 五 meta+docTitle/docAuthor=anonymous+目录 navPoint playOrder=0+章节
  navPoint "N. 标题"）/章节 xhtml 头模板（charset+title+style.css link+h1）+
  container 单行形。
- **转义策略拆分**：`escape_html`（五字符实体，标题/navLabel/OPF 元数据面——参考
  产物中 CJK 原样保留）与新增 `escape_numeric`（非 ASCII → `&#xNNNN;` 大写十六进制
  +ASCII 原样，仅段落 `<p>` 内容使用——参考产物 0_.xhtml 实证）。
- 不变量：uuid v4/`dcterms:modified` 秒级/`dc:date` 毫秒级/版权年份=运行时生成
  （TS 同为运行时值）；golden 测试以 regex 归一后对比。

## 三、golden 测试与必红

- `epub_matches_ts_reference_structure`：fixture
  `tests/fixtures/ts-epub-reference.epub`（TS 同输入产物固化）解包对比——文件名集
  （双章 3 xhtml）/mimetype 逐字节/container 逐字节/style.css 逐字节/OPF 归一化后
  骨架行序列（CONTENT_ITEM_A/B、CHAPTER_FILE_A/B 占位）/NCX navPoint 数与 head/
  docAuthor 面/章节头模板逐字节/段落 CJK 数字引用（`&#x6797;`）。
- **必红**：NEUTER974 中和 `escape_numeric` 数字分支 → golden 恰 1 红（CJK 段落
  无数字引用），撤除后绿，残留 0。

## 四、真引擎活体（8974，二进制重建后核验）

- API：upload→create→export epub → 产物 `works/<id>/source/exports/山河之书.epub`
  九文件、mimetype STORED、OPF v3+nav+guide、NCX 目录 navPoint；`exportSourcePaths`
  断言（971 号）；`debug/tools` 33 条含 revise_paragraph（973 号回归）。
- 环境清偿：引擎杀净、临时根删除。

## 五、门禁与插曲

- clippy 双 0；testgate engine-rs 2109（+1）/src-tauri 584（INKOS_DUEL=1）+audit:rust
  双 0+必红自检活体；gate:ts 十步全绿（diff 78/0）；U+FFFD 净。
- 插曲：zip 9.4 `entry.name()` 返回 `ZipResult<Cow<str>>`（非直通 &str）——unwrap 后
  再用；roundtrip 旧断言（chapter-0001.xhtml 路径+实体转义）随新结构更新（数字引用
  双轨断言）。

## 六、备案

- `epub-gen-memory` 段落内容数字引用的精确实现位（bundle 内部）未逐行追溯——以
  fixture 产物为 golden 行为锁（同输入同结构），实现面差异不影响产物一致性。
- TS 时间戳毫秒/秒双形与 uuid 均为运行时值；产物字节级全等不成立（也非目标），
  结构级 golden 已覆盖。

**下一号自 975 起；出口=C 类 SSE 残余项逐事件定性收编 / registered-sources 进代理
确认卡；用户侧待办=Fork 推送（含 955-974）。**
