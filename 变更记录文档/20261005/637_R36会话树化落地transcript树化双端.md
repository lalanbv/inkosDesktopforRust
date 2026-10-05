# 637 号：R36 会话树化落地——transcript 树化双端（v6 路线最后一项待实施工程）

- 日期：2026-10-05
- 类型：feat(engine+core)+chore(smoke)
- 路线：v6/Pi 对标 R36（546 施工图 §3 设计稿→本轮实施）；603 号待裁决清单「R36 储备」的开工清偿
- 编号：原编 636，写档前三查发现并行会话已落库 636 号（4e20e9b9，门禁复验+GUI 走查）——按 534/536/541 先例让号改 637
- 状态：✅ 收口

## ① 缘起与价值判定（对准真实需求）

R36 是 v6/R30–R37 路线唯一未实施项（R30b/R32/R33/R37/R38–R46 均已落库；R30 终迁因 key 不可得维持缓行，本会话第五轮 env 复测仍无 key）。分支价值在**创作分支**（546 §3.1）：章节重写回溯（弃稿不丢）、方向 A/B 对照、节拍/结局多分支试写——「从第 N 轮重开但保留弃用路径可回看」。request 三事件 + message uuid/parentUuid 既有原料使树化成本低于上游当年 v1→v2 重构。本轮按施工图开工清单五条实施（studio 呈现按 §3.5 边界不做，另立项）。

## ② 设计落地（零版本增量加法式，546 §3.2 全兑现）

- **schema 加法**：TS `BaseEventSchema` 加 `parentSeq: int.nonnegative().nullable().optional()`；新事件变体 `branch_moved { fromSeq: number|null, toSeq: number|null }`（toSeq=null 等价 resetLeaf）；版本字面量保持 `z.literal(1)` 不动。Rust 镜像：全变体加 `parent_seq: Option<Option<u64>>`——**三态缺一不可**：`None`=键缺省（legacy 线性链）、`Some(None)`=显式链根（null）、`Some(Some(n))`=父 seq（TS `undefined|null|number` 逐态镜像）。
- **head replay**：`transcriptHead(events)` 按 seq 序重放——branch_moved 置 head=toSeq（null=置空），其余事件推进 head=seq。与 Pi「leaf 不落盘、靠 branch 后必 append」不同：本仓会话是服务端长生命周期对象（跨引擎进程重启、双引擎切换），branch_moved 落事件流、head 从流重建（O(n) 装载路径一次）——跨进程持久（546 §3.3）。
- **active 链回溯**：`activeChainEvents(events)` 从 head 沿 parentSeq 反向回溯 root→head（升序=seq 序路径子集；父 seq<子 seq append-only 不变量）。键缺省（legacy）语义=父即 seq 序前一事件——纯旧文件退化为全量（**旧行为零改写**）；链断保留已收集后缀（553 号 compaction 容错先例）；branch_moved 自身不入链（元事件只改写 head）。
- **戳记收拢**：parentSeq 由双端 append 助手统一戳记（`appendTranscriptEvents`/`append_transcript_events`：branch_moved 戳 head 后改写 head；其余事件戳 head 后推进）——链语义单一事实源，全部既有写入方（agent-session 流式/manual/compaction/metadata/migrate/production）**零改动**获得树化语义。
- **读面路径化**：恢复只看 active 路径——TS `restoreCommittedDialogueScan`/`deriveBookSessionFromTranscript` 与 Rust 双镜像入口做链过滤；compaction 随链生效（分支各自的压缩边界互不污染，546 §3.4 的 Pi 红利兑现）；**元数据保持全局扫描**（title/bookId/kind 跨分支共享，会话级状态不入分支语义）；「未 committed 的 request 分支在回溯时自然剪除」按设计语义成立（分支到未提交请求中点=该请求消息不回放，单测锁定）。
- **REST 最小暴露**：`POST /api/v1/sessions/:id/branch { toSeq }`（缺省/null=resetLeaf）→ `{ ok, head, branchCount }`；400 INVALID_BRANCH_TARGET（非法/不存在 seq）；404 平铺；**409 SESSION_BUSY**（生产任务控制器+聊天互斥队列双查——保证单请求事件整体落在同一链上；TS 新增 `isAgentSessionBusy` 无副作用查询，对照 abortAgentSession）。GET 列表/详情透出 `head`/`branchCount`（derive 恒带键对齐双端差分）。

## ③ 红绿可证伪

- TS：新测 `session-branch.test.ts` 12 断言（append 戳记线性链/legacy 旧文件零改写解析+继续追加链延展/单分支剪除弃用段+branch_moved 不入链/多分支整段弃用重写/分支到未提交请求中点消息剪除/resetLeaf 新链根/重启 replay/无效目标+空会话/压缩边界互不污染/derive 透出/branch_moved 行 golden/新写入行恒带 parentSeq 键）。**旧码 stash 红 12/12**（parentSeq undefined+branchBookSession 不存在）→ 新码绿。
- Rust：`book_session_store::tests` 9 断言镜像四态+压缩隔离+JSON 形态（camelCase/parentSeq 恒键）。真回归抓手：`agent68_e2e::history_replay` 旧语义直写 transcript 在新码下红——暴露「Option<Option> 序列化 None 亦产出 null」伪象（见 ⑤-3），修 write_transcript 剥离 parentSeq 键后绿。

## ④ 双端与门禁面固化

- smoke：node-fallback-smoke 新增 branch 探针 8 断言×双腿（建会话→驱动一轮→读盘验 parentSeq 恒键→无效目标 400→branch 200 head/branchCount→第二轮链回分支点→derive active 路径+透出→未知会话 404）——node+rust 腿 16/16 全绿（rust 腿先重建 debug 二进制再判读，632 号教训第四次预判规避）。
- 差分器：会话面新增 branch 维度（双腿各 branch 回首轮提交点→驱动第二轮→per-leg 链完整性→跨腿 derive 深比对）；head 为绝对 seq、metadata_updated 计数豁免可致 seq 漂移——branch 比对面 head 按 volatile 剪除（branchCount/消息面结构等价仍硬比，备案）。对照 60 端点分歧 0。

## ⑤ 插曲四条

1. **撞号让号**：写档前三查发现并行会话已落库 636 号（4e20e9b9）——本号 637。三查纪律再次止损（当日第二例：636 本身也是并行落库）。
2. **serde 双 Option 语义坑**：`Option<Option<u64>>` 反序列化时 JSON `null` 默认塌缩为外层 `None`（与键缺省不可分）——树化三态语义失效。`deserialize_with` 手写双层包裹（缺省走 default，在场包 Some）修复；序列化侧 None/Some(None) 均出 null 无歧义（写入恒经戳记，None 不会落盘）。
3. **批量插入脚本污染 match 臂**：python 批量补 `parent_seq: None,` 时把 match 臂模式行（`TranscriptEvent::Message {` 结尾）也当构造点插入——模式变成只匹配 parent_seq 缺省的事件，`as_message_ref` 等四处静默失配（agent68 回放面消息投影恒空）。教训：AST 级改动不要用行尾 `{` 正则猜构造点；编译器只报语法不报语义，回归靠既有 e2e（agent68）当场抓获。
4. **legacy 模拟的序列化伪象**：直写 transcript 的测试用结构体序列化会带出 `"parentSeq":null`（=显式链根），与「legacy 文件无该键」意图相反——write_transcript 剥键修正。语义提示：新写入恒带值（null=链根）与 legacy 缺省（=线性前驱）在字节面可分辨，这正是三态设计的意义。
5. **duel 首跑 9/10 flake**：`bin_process_static_face_duel` 首轮红、单跑隔离即绿、全目标复跑 10/10——负载性启动超时 flake（627 号先例第三次实证），非本号改动面（静态面路由零变更）。

## ⑥ 门禁

- clippy 双 crate `--all-targets` 0 警告；
- cargo:testgate（INKOS_DUEL=1 脚本内注入+audit:rust 前置双 0）：engine 45 目标 **1915 passed(+9)**、src-tauri 25 目标 **584 passed** 全绿（落盘日志解析）；duel 真跑逐名复跑 **10/10**（首跑 1 例负载性 flake 隔离复跑绿收口，见插曲 5）；
- gate:ts 七步全绿（build/typecheck/test/audit:npm/smoke 含 branch 探针双腿 16 断言/差分 60 对照 0 分歧/epub）；
- bench:gate 低谷通过（首跑负载闸 24%/核拦截照 529/631 先例候谷复跑 3.33%/核通过；14 基准全阈内零回退——链回溯 O(n) 在装载路径非热路径，546 §5 预核成立）。

## ⑦ 备案与边界（不做清单兑现）

- studio 呈现（分支切换器）按 546 §3.5 另立项；本轮 GET 透出的 head/branchCount 即未来 UI 消费面。
- 跨文件会话谱系（Pi parentSession）/labels/context_edit/custom/thinking_level 条目类型：不做（无消费方）。
- legacy-only 会话（.json 未迁移窗口）经 loadBookSession 自动迁移后 derive 恒带 head/branchCount 键；纯 .json 读取窗口（迁移失败时）TS 侧缺键 vs Rust 侧 null——极窄窗口备案（差分器活体面不可达）。
- pi 1.0 终迁：key 第五轮复测仍不可得，缓行维持（633 号净工作清单不变）。

## ⑧ 教训

1. 加法式树化的关键是「读面退化守恒」：无 branch_moved 的文件链回溯必须严格退化为旧行为——旧行为零改写不是迁移策略而是语义定义（本轮全部既有 2268 TS 测试+1906 Rust 测试零改动通过即证）。
2. 三态语义（absent/null/值）跨语言对齐时，serde 默认行为是第三态杀手——null 塌缩必须在反序列化层显式修复，序列化层无条件补齐（写入恒经单一收拢点）。
3. 正则批量改枚举构造点会静默污染同形 match 臂——「编译过≠语义对」，enum 加字段时所有同形模式的甄别要靠人审或 AST 工具；既有 e2e 的历史回放断言是最后一道网（本轮真实接住）。
4. 双端活体差分要把「绝对序数」与「结构等价」分开裁断（head vs branchCount）——豁免要写明因果（metadata_updated 计数豁免→seq 漂移→head 漂移），否则豁免变漏洞。
