# 204 号：Box::leak 迁移收官——全仓代码泄漏归零

- 日期：2026-09-07
- 分支：develop
- 关联：202 号（量化与双链重构）、203 号（books_routes 主力面）
- 推送核验：origin/develop 仍停 6bc65564——**201–203 三提交（97dfa56f/7b17ad2f/c4b9cb85）均未推送**；本批次后本地领先 4。

## 一、范围

按 203 号同模式迁移余下全部代码泄漏：book_create_routes 15 处（指令主体）+ 顺势并入 fanfic_routes 5、ops_routes 4、interactive_film_routes 2（指令「酌情并入」）。**202–204 三批次累计：全仓 Box::leak 代码 66 → 0**（src 树仅余 /// 文档注释中的字样）。

## 二、实施

### book_create_routes（15 处，四组 + 签名泛型化）

- 中间函数 `generate_and_review_foundation_multi` / `generate_and_review_foundation_import` 签名 `&'static RoutedAgent` → `&RoutedAgent`（同帧借用，2 处）。
- create 主链（architect + reviewer）、import 首章（architect + 分支内 reviewer）、revise_foundation（architect + reviewer）→ 局部值 + `&` 传参（6 处 leak 消除）。
- 回放链：analyzer_chat 局部值；analyzer_ctx/writer_ctx 两套 `Box::leak` ctx（8 处）→ `AgentCtxPorts` 借用——`AgentCtxPorts` 新增 `analyzer_ctx()` 视图（ChapterAnalyzerCtx 同为「2 路径」形态）；`save_chapter` 用 `writer_ctx()` 视图。

### fanfic_routes（5 处）

- `review_loop` 内 reviewer、两处 importer → 局部值 + `&`。
- **fanfic/spinoff 两处 architect 闭包**（`|feedback| Box::pin(async move ...)` 传给 `impl FnMut -> Pin<Box<dyn Future + Send>>`）：`dyn Future + Send` 默认 `'static` 约束——闭包不能搬 `&RoutedAgent` 引用。改捕获 `Arc<AgentRouter>`（`architect_router.clone()` 浅 clone），async 块内构造局部 `RoutedAgent`（review 每轮重生成一个栈值，零泄漏、future 'static 成立）。

### ops_routes（4 处）

- detect 自动改写链：reviser_chat 局部值 + `AgentCtxPorts::reviser_ctx()` 借用（`detect_and_rewrite` 签名本就是 `&dyn ReviserChat` + `&ReviserCtx<'_>`，零适配）。

### interactive_film_routes（2 处，非 ctx 形态)

- 两处响应头 `Box::leak(format!(...).into_boxed_str()) as &str` → owned `String`（`String: TryInto<HeaderValue>` 成立；`CONTENT_TYPE` 元素同步 `to_string()` 统一数组类型）——导出文件名响应头不再泄漏。

## 三、验证

- 全门禁：engine lib **1277** / 集成 **194+70+10**、clippy --all-targets -D warnings 零告警、bench 门禁 ✓、INKOS_DUEL=1 strangler_duel **10/10**。
- 真机冒烟（生产形态 bin + fixture）：health 200；`POST /books/b1/fanfic/refresh` → 装配链走通、失败在 LLM 不可达（非 panic）；`GET /projects/p1/export` → 404（项目不存在，路由与 header 路径正常无 panic）；进程存活。
- 纯 Rust 端改动（core/studio 无对应面）。

## 四、结论

**Box::leak 债务清偿完毕**（202 量化+write-next 双链 → 203 主力面 31 → 204 收官 26）：引擎装配层全部改为「owned 持有者 + 同帧借用」或「局部值 + & 传参」，值随调用帧整体 drop；router 全链共享 `Arc`。唯一保留的 `'static` 形态是 `WriteNextAgents<'_>` 消费面与闭包 future 的 `'static` 约束本身（语言要求，非泄漏）。
