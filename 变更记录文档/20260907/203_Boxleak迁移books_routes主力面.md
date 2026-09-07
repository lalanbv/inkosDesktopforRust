# 203 号：Box::leak 迁移（续）——books_routes 主力面 31 处归零

- 日期：2026-09-07
- 分支：develop
- 关联：202 号（write-next 双链重构与量化）、201 号（路径段守卫）
- 推送核验：origin/develop 仍停 6bc65564——**97dfa56f（201）与 7b17ad2f（202）均未推送**，本地领先 2；本批次后领先 3。

## 一、范围

按 202 号 backlog 同模式（owned 持有者 + 同帧借用）迁移 books_routes 剩余 **31 处**泄漏（plan/settle/revise/rewrite/retry/compose/consolidate/repair/resync 九组端点）。

## 二、实施

### 基础设施

- **`FsStateStore` 全局静态**（state/store.rs）：`pub static FS_STATE_STORE: FsStateStore = FsStateStore;`——FsStateStore 是 ZST（单元结构体，无状态），`&FS_STATE_STORE` 是真 `'static` 引用（零泄漏零分配），取代各端口装配处 `Box::leak(Box::new(FsStateStore))`（旧泄漏虽是零字节分配，语义仍是泄漏）。
- **`AgentCtxPorts`**（books_routes.rs）：低频端点 ctx 的 owned 持有者——路径 2 个 PathBuf owned、store 走全局静态；`writer_ctx()`/`reviser_ctx()` 借用视图随帧 drop。

### 九组迁移（books_routes.rs）

| 端点/位置 | 旧形态 | 新形态 |
|---|---|---|
| plan（216） | leak RoutedAgent | 局部值 + `&planner` |
| settle（286-292） | leak writer + WriterCtx×4 | 局部 writer + `AgentCtxPorts` |
| revise（788-793） | leak reviser + ReviserCtx×3 | 局部 reviser + `AgentCtxPorts::reviser_ctx` |
| revise 尾 settle（822） | leak writer | 局部值（ctx 既有静态提升形态换 FS_STATE_STORE） |
| resync retry_settler（897-905） | WriterCtx×4 leak | `AgentCtxPorts`（RoutedSettler<'_> 借用形态） |
| compose 内 plan（1195） | leak planner | 局部值 |
| compose（1219） | leak composer | 局部值（selector/compiler 借用同一值） |
| consolidate（1270） | leak consolidator | 局部值 |
| RepairSettle::settle（1347） | WriterCtx×4 leak | 局部 `AgentCtxPorts`（self 字段 clone 构造） |
| resync 尾 save（1811） | WriterCtx×4 leak | `AgentCtxPorts` |

books_routes 代码泄漏 **31 → 0**（全仓代码泄漏 58 → 26；其余 grep 命中为文档注释）。

## 三、验证

- 全门禁：engine lib **1277** / 集成 **194+70+10**、clippy --all-targets -D warnings 零告警、INKOS_DUEL=1 strangler_duel **10/10**。
- 真机冒烟（生产形态 bin + fixture）：health 200；`POST /books/b1/plan` → 装配执行成功走到 **LLM 阶段**（错误为 LLM 不可达 127.0.0.1:9，非 panic）；`POST /books/b1/settle` → 走到 genre 解析业务错误（非 panic）；进程存活。
- 纯 Rust 端改动，Node 回退端无对应面。

## 四、遗留 backlog

- 全仓剩余 **26 处**代码泄漏：book_create_routes 15、fanfic_routes 5、ops_routes 4、interactive_film_routes 2——均为低频手动端点，同模式迁移（`FS_STATE_STORE` 静态 + 局部值/`AgentCtxPorts`）。
