# 962 号 · SSE 事件面逐名定性轮：未入名单 26 名分类 + Rust 衍生/仿写创建失败补发 book:error

> 961 附带备案出口：server broadcast 名与 studio 全局事件名单 `STUDIO_SSE_EVENTS`
> 求差。本轮逐名定性（双端发射面 × UI 引用面 × 订阅通道），定型 1 处双端漂移并
> 修复：Rust 衍生（spinoff）/仿写（imitation）后台创建失败只发领域错误、不发
> `book:error`（TS 同点位双发）——全局名单只收 `book:error`，Rust 默认后端下创建
> 失败对书目刷新面不可见。
> 验证：Rust e2e fanfic59 3/3（+1）+testgate engine-rs 2091（+1）/src-tauri 584+
> audit:rust 双 0（自检活体）+clippy 双 0+gate:ts 十步全绿+U+FFFD 净。

## 一、求差与逐名定性（TS 72 名 / Rust 71 名 / 名单 49 名 → 未入名单 26 名）

| 类 | 事件名 | 定性 |
|---|---|---|
| A 聊天流专道 | agent:aborted、context:compression、request:snapshot（仅 TS）、thinking:start/delta/end、tool:start/end | 聊天 store `stream-events.ts` 经 `subscribeStudioEvents` 逐名直订（不走全局名单）——设计内分流 |
| A′ 旧轨死分支 | tool:start/end（BookSidebar.PanelView 读全局 `sse.messages`） | 小屏抽屉「正在审计/修订中」指示永不触发；PanelView 为 P3-4 标注的待删旧轨，且把 tool:* 灌入全局环形缓冲会波及约 16 个 `sse.messages` 消费方——**备案不动** |
| B 同点位配对 | spinoff:start/complete/error、imitation:start/complete/error | 双端成功均配发 book:created；失败 TS 配发 book:error，**Rust 缺发——本轮修复** |
| C 无 UI 消费方 | consolidate:*、director:*、foundation:revised/error、repair-state:*、chapter:deleted/restored | 发起操作的按钮处理器自行 refetch；经聊天/代理外部触发时已打开的书籍视图不刷新——候选纳入刷新集，需逐事件核负载与频度，**备案** |
| D 内部 | engine:shutdown（仅 Rust） | 停机信号，无 UI 语义 |

## 二、修复（Rust fanfic_routes.rs）

- `spinoff_init` 后台失败分支：`spinoff:error` 后补 `book:error`（同 bookId/error
  负载，TS server.ts 同点位顺序）。
- `imitation_init` 后台失败分支：`imitation:error` 后补 `book:error`。
- 成功分支本已配发 `book:created`（负载 Rust 仅 bookId、TS 附 book 摘要——全局
  消费方只用事件名触发刷新，不读 book 字段，MINOR）。

## 三、测试与必红

- 新例 `spinoff_and_imitation_failures_also_broadcast_book_error`：LLM 不可达 →
  订阅总线等到领域错误事件，断言紧随其后的下一事件为 `book:error` 且负载逐字相同；
  衍生与仿写双路径。
- **必红**：stash `fanfic_routes.rs` → 恰该例红（领域错误后无后续事件），pop 后
  3/3。

## 四、门禁

- clippy 双 0；testgate engine-rs 2091（+1）/src-tauri 584（INKOS_DUEL=1）+
  audit:rust 双 0+必红自检活体；gate:ts 十步全绿；U+FFFD 净。

## 五、结论

全局事件名单与双端发射面的差集已逐名定性（8+6+11+1=26）：设计内分流 8 名（其中
tool:* 2 名另在旧轨 PanelView 有死分支，备案）、同点位配对 6 名（Rust 失败配发
缺口已补齐）、无消费方 11 名备案、内部 1 名。

**下一号自 963 起；出口=C 类刷新集逐事件定性 / 翻译域 2.0 全量迁移规划 / 写法
绑定 B 面；用户侧待办=Fork 推送（含 955-962）。**
