# 440 号：R21 Context Lens（上下文透视）面板真机走查 + walkthrough-fixture 同名建书/复用根双加固

- **日期**：2026-09-15
- **类型**：test(walkthrough) —— UI 验证缺口收口 + 走查数据面加固
- **关联**：391 号（R21 Context Lens 契约+接线，此前仅单测/golden/bindings 证据）、437 号（walkthrough-env 编排）、438/439 号（确认卡降级态与失败注入）
- **提交**：本记录 + `scripts/walkthrough-fixture.mjs`

## 1. 背景与选题

437–439 号建立了「一键走查环境 + 浏览器真机验证」闭环后，盘点四轮 P0 面板中从未真机验证过的面：R21 Context Lens 面板（`packages/studio/src/components/ContextLensPanel.tsx`，挂书籍设置页）只有 golden 向量+端点行为测试+tsc/bindings 证据，浏览器渲染从未看过。本循环补上这块验证缺口。

## 2. 走查方法与证据链

环境：`node scripts/walkthrough-env.mjs --root /tmp/inkos-walk440`（mock 1234 + 引擎 8787 + fixture；引擎二进制 23:25 > engine-rs 末次变更 23:12、studio dist 00:28 > 源码 23:54，双端免重建）。

### 2.1 有数据态（fixture 书，write-next 章 2）

API 基准 `GET /books/镜花水月/context-lens/2`：12 条来源 / 受保护 6 / 1800 tokens / compression=null / notes=[]。

浏览器（书籍设置页滚动到面板）DOM 提取 + 截图比对：**12/12 零漂移**——
- 汇总行逐字：「12 条来源 · 受保护 6 · 约 1800 tokens · 预算内未压缩」（compression=null 走未压缩分支 ✓）
- 12 条目顺序/来源（等宽字体）/层级标签（本书事实×3、本书规划×3、本书时序记忆×5、写法资产×1）/token 数逐条对齐
- 6 个「保护」徽标恰好落在条目 1–6（emerald 底）；无「编译产物」徽标（compiledEntries=0 ✓）；无压缩留痕块（compression=null ✓）；无备注行（notes=[] ✓）
- 章节选择器唯一选项「第 2 章」，value=2（工件只在 write-next 产出的 0002 章；0001 为手工预置无运行留痕，符合预期）
- `entry.rank` 字段 UI 不渲染——纯展示取舍，备案非缺陷

### 2.2 空态（walkthrough 中段书被 mock 建书重置后的 0 章 0 工件态）

面板渲染：无选择器 + 斜体提示「暂无装配留痕——写一章后可回放该章上下文装配清单。」——与设计空态分支逐字一致（DOM 提取 + 截图双证）。

### 2.3 加固后 13 条新基线

fixture 补写 story_bible.md 后（见 §3.1），lens 实测 **12→13 条**：新增受保护来源 `story/story_bible.md#story-bible`（本书事实层，order 2，~84 tokens），totals 13/7/1884。浏览器复核 **13/13 零漂移**（汇总行逐字+story_bible 条目徽标位置正确）。UI 在两套基线下均与 API 完全一致。

**未覆盖**（备案）：压缩留痕分支（需超预算章，golden 向量已覆盖投影正确性）；错误路径（列表拉取失败回退空态文案——设计如此，非缺陷）。

## 3. 走查中发现并修复的两个 fixture 真问题

### 3.1 同名建书静默清空 fixture 数据（已加固，实测 409）

真机走查建书链时发现：mock 建书（title=镜花水月）成功后 fixture 书的章节与 runtime 工件被整体清空。溯源：

- `book_create_routes.rs` L281-289：目标目录存在且 `complete_book_exists` → 409 拒绝；**存在但不完整 → `remove_dir_all` 整目录删除后 staging rename 重建**（刻意的「不完整目录重建」语义，TS 侧 `completeBookExists`（server.ts L1700）同判定同 409 同擦除——双端一致，非产品缺陷）。
- 完整性 = `book.json + story/story_bible.md` 双存在。fixture 写了 book.json 但**漏写 story_bible.md** → 被判不完整 → 被重建清空。

修复：fixture 补写 `story/story_bible.md`。副作用如实记录：该文件在场时会以受保护来源进入 write-next 装配（12→13 条），对 mock 走查无碍但基线变了——注释已写明「勿再用旧基线断言」。加固后实测：同名 `POST /books/create` 返回 **409 `Book "镜花水月" already exists`**，fixture 数据不再可能被静默清空。

### 3.2 复用根二次起停死于 resync（已修复：只读跳过路径）

fixture 声称幂等（env 头注「已存在则复用」），但 resync/1 在 write-next 章已落盘时被拒：`Only the latest persisted chapter can be synced safely (latest is 2)`——env 二次起停会直接 fixture 失败退出（437 号遗留缺口，此前只验过文件写幂等，没验过链段重放）。

修复：链段前加守卫——`GET /books/:id` 的 `nextChapter ≥ 3`（write-next 章已存在）则跳过 resync/write-next，直入只读断言。实测重放全绿（kinds 四类齐、run-log 断言过、exit 0）。附带收益：重放不再重跑 settle 链，此前「脏根重放 kind 不全 ⚠ 警告」的误导场景随之消失。

## 4. 走查操作备忘（沿用 439 号 + 新增）

- **发送按钮定位必须用 `compareDocumentPosition`**：`btns.indexOf(ta)` 恒为 -1（textarea 不是 button），导致点到文档里第一个无名按钮（浮动球）——消息看似发出实则从未离开前端（run-log 零增长暴露）。正确姿势：`ta.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING` 过滤出 textarea 之后的无名未禁用按钮。
- 建书确认卡点「继续执行」后约 12s 内完成（mock 架构师链），UI 自动切到新书会话页。
- 书籍设置页面板栏滚动：`scrollIntoView({block:"center"})` 对 h3 根本元素有效；多滚动容器手算 scrollTop 会滚过头，截屏前以 `getBoundingClientRect().top` 复核。
- 本会话浏览器内核每调用全刷新，`tab` 绑定必须每次重建（bootstrap + `tabs.get`）。

## 5. 门禁

- `node --check scripts/walkthrough-fixture.mjs` ✓
- fixture 端到端四跑：干净根全链（kinds 四类齐 ✓）/ 脏根重放（复现 resync 拒绝，锁定缺口）/ 守卫后重放（跳过路径全绿 ✓）/ 断言路径再验 ✓
- 同名建书 409 实证 ✓；加固前后 lens API+UI 双基线零漂移 ✓
- 无 Rust/TS 产品源码变更，cargo/pnpm 门禁不适用；bench 无涉及

## 6. 遗留与提醒

- **待推送**：433–440 号共 8 笔本地提交，请用户在 Fork 图形端推送后核验 origin/develop。
- 压缩留痕分支（面板琥珀色块）真机渲染仍未见过——需造超预算章（真实长跑数据驱动），与 R16/R19 同批观察。
- product backlog 下一个编号自 **441** 起。
