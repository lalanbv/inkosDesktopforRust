# 987 号：全量验收轮（代码审查+全量测试+模拟真人走查）——rcm 分序列潜伏红清偿 + UI 写作链审计端口错配修复 + 走查全链活体实证

> 日期：2026-11-12（目标=全量测试/代码审查/模拟真人测试/确保无 BUG——同 706 号验收轮形态）
> 范围：scripts（rcm）/ engine-rs（books_routes 装配宏）/ engine-rs tests（e2e_write_next_contract mock 配对面）/ gui-test-screenshots（走查留档）
> 双潜伏红均非本轮改动引入：rcm 红自 815 号起、UI 假阴性自装配缺省面引入起——均为「合并/特性大轮后契约套件未复跑」的跨轮潜伏（706 教训第二次复发实证）。

## 一、代码审查（984/985/986 新增面独立复审——零缺陷）

逐 hunk 复审近三轮新增代码：

1. **bounded-map.ts（985）**：delete→size>=cap 逐最旧→set 三步序正确（覆写先删不触发逐出=不增长语义；cap=64 两调用点；四例锁形测试覆盖无逐出/最旧逐出/覆写保鲜/cap=1）。
2. **chatRequests finally 终态删除（985，server.ts:6616）**：恒等守卫 `get(sessionId)===request` 覆盖三态——abort 路径先删（get=undefined 不误删）、retry 校验失败恢复形（throw 在 try 外不经过 finally+map 已 restore）、并发竞态（新请求覆盖后旧 finally 不删新条目）。读面兜底逐点核验（loadChatRequest 双检/retry previous ?? store.load/busy 在飞判）。
3. **pending_session_aborts cap（984，agent_route.rs）**：同锁作用域内 len>=cap && !contains_key → min_by_key(seq) 逐最旧 → insert——整段同锁无窗口；seq 源 AtomicU64 fetch_add 全序无并列；覆写路径不逐出（contains_key 前置）；测试 80 探针双断言并发鲁棒（last 必可达+hits≤cap，seq=0 边界 78=79−1 解明）。
4. **a11y 三站（986）**：TruthFiles aria-label `tr()` 直用与 StoryGraphTree:191 先例逐字同款（lib/app-language 全局语言；ja 回退 en 为 219 号既定口径非漂移）；harness API_DATA 扩面与 useApi 消费面同构。

## 二、全量测试（baseline 至终态全绿）

| 门禁 | 结果 |
| --- | --- |
| gate:ts 十步 ×2（改动前初跑 + 改动后终态） | 全绿（终态 build 19.8s/typecheck 17.9s/test 102.9s/audit:npm 2.8s/rust-bin 0.2s 活体同源/node-fallback-smoke 35.9s/engine-contract-diff 36.4s/export-epub-smoke 35.4s/work-alternation 5.1s/check-routes 0.1s） |
| clippy:gate | engine-rs 0 + src-tauri 0（--all-targets 含测试面） |
| cargo:testgate | engine-rs 52 目标 **2113 passed**（+1=987 锁形测试）/ src-tauri 25 目标 584 passed（INKOS_DUEL=1 真跑） |
| audit:rust | 双 lock 0 漏洞 + 必红自检 7+1 活（yanked 联网核验超时 213/336 次=网络抖动；本轮 testgate 内 audit 一次瞬态失败（engine-rs 段未打印即退）——单独复跑全绿 exit 0，定性网络抖动非漏洞） |
| review-cycle-matrix | rust 腿 **7/7 严格全绿 exit 0**（修复前 7/7 硬红——见 §三）+ node 腿软失败记账 exit 0（695/700 既定口径） |
| U+FFFD | 三改动文件净（books_routes.rs / e2e_write_next_contract.rs / review-cycle-matrix.mjs） |
| bench | 不重跑——983 号基线 2026-11-01 采集有效（本轮全门禁内无 bench 触发面，严禁 --update 洗基线纪律） |

**并行会话备注**：本轮执行期间工作树出现非本轮改动（package.json / src-tauri secrets/sync.rs / src-tauri tests/sync_watcher_io.rs / scripts/cargo-ignored-sweep.mjs——开工三查时树净，系并行会话中途写入）。cargo:testgate 的 src-tauri 584 以并行共存工作树编译为准（其改动随编译入测且全绿）；本轮提交严格路径限定（git commit -- <paths>），并行物不入本轮提交。

## 三、潜伏红一：rcm rust 腿 7/7 硬红——815 号 resync 审计段消费分序列

**暴露**：本轮 review-matrix:rust 实跑 7 场景全 ✗（32-37s/场景）——自 706 号（10-15，rust 腿 7/7 全绿）后该套件未复跑，815 号（11-02）resync 审计段落地即成潜伏红。

**根因（证据链）**：815 号给 `POST /resync/:chapter` 加 `record_resync_audit`（FullCycleAuditor 真实审稿，对齐 TS resyncChapterStateAndAudit 副作用语义）→ fixture 流 = resync/1（**消费一枚 mock 降分**）→ write-next（审核环）。walkthrough-mock 的 `AUDIT_SCORES` 递增序列假设审核环是唯一审计消费者（646 号设计时成立），右移一位后逐场景推演全部吻合：B [45,91]→resync 吃 45、审核环得 91→无循环（日志行全缺+落盘无 PATCH）；G2 [45,55,55,91]→审核环 55→修复轮 1→复审 55→**未净提升（55→55）**→PATCH1 被弃（落盘断言红）；G 双 PATCH 缺 PATCH3…… 7/7 假红。

**修复（测试基建面，产品零触碰）**：rcm 场景 spawn 时分序列统一前置 resync 占位枚（`RESYNC_AUDIT_PLACEHOLDER_SCORE="45"`——<85 任意值皆可，resync 审计只沉淀 review_metrics 不影响审核环裁决；取 45 与场景首分对齐便于日志判读）+ 套件头注释同步（665 教训）+ 场景表保持 write-next 相位语义不变。**双腿统一前置**：TS 2.0 resync 同样审计（resyncChapterStateAndAudit），fork 契约恢复严格化时同样需要——node 腿软记账模式下无判定影响。

**红→绿**：修复前 7/7 ✗（本轮取证）→ 修复后 rust 腿 7/7 ✓ exit 0（32-42s/场景）；node 腿软记账 exit 0 维持。

## 四、潜伏红二：UI 写作链审计端口错配——write_next_assembly! 缺省 full_auditor=None

**暴露（模拟真人走查）**：walkthrough-env rust 腿 + IAB 黑盒点「写下一章」——全链完成但任务**失败**态：「已为 镜花水月 写出第 3 章……但审稿未通过，状态 audit-failed」。inkos.log 地面真值：`修复轮次 1/1（当前 50 分）`→`未净提升（50→50）`——**50 分=RoutedAgent 文本协议审稿的回落签名**（PASS/FAIL 行协议，JSON 契约响应无 PASS 行→passed=false+score 50，agent_router.rs:580），而非 mock 默认 88。

**根因**：`WriteNextAgentPorts::agents()` 自 01f2e259 起 `full_auditor: None`——审核环回退最小文本协议审计（write_next.rs:849「44 号：完整审计器主链；缺省回退最小协议」的缺省面）。三条生产装配面全中招（`write_next_assembly!` 宏调用点）：agent 路由 `execute_write_next`（**UI 写下一章按钮**）/ ops 守护 `write_one_chapter` / sub_agent_tool 写作工具。同实体三端口不一致：bin 装配层（123 号手写展开挂有效 router）、审计路由、resync（815 号）均 FullCycleAuditor JSON 契约，唯生产装配宏缺省——815 号教训「CycleAuditor 两实现 文本协议 vs JSON 编排 错配即假阴性」的现役实例（815 号当时已实锤同族假阴性 ch1 50/false）。产品影响：真实 LLM 下 UI 写作链跑的是单维文本审稿（PASS/FAIL+末行分）而非 12 维结构 JSON 审计（issues/repair_scope typed 路由），review_metrics 沉淀 50/90 粗分污染质量趋势。

**修复（宏级单点收敛）**：`write_next_assembly!` 宏内统一装配 `full_auditor=Some(FullCycleAuditor{effective_router, project_root, builtin_genres_dir, 占位 book_dir/chapter_number/genre})`——与 bin 手写展开逐字段同构（for_chapter 在 write_next_chapter 内按章重绑 book_dir/chapter_number/genre；project_root/builtin_genres_dir 供 AuditChapterCtx 读取）。四个调用点（含 run_draft Manual 模式——装配无害）一次对齐。**锁形测试** `write_next_assembly_fills_full_auditor`（books_routes tests，先例同 write_next_ctx_installs_timeline_beats_port）：必红=暂回退 HEAD 无界形恰红「full_auditor must be wired by assembly (987)」→精确互逆恢复复绿。

**配套 e2e 契约面镜像（7 处 mock+装配）**：文本协议审稿面退役后，配对 mock 的审稿分支须回 645 JSON 契约——新增 `AUDIT_JSON_PASS` 共享常量（passed:true/overall_score:88/issues:[]），七处 "PASS\n95"/else-"PASS" 文本面统一替换：顶层 spawn_mock_llm / mock58_llm / ops72 mock_llm / mock_sub87_llm / mock_detect_loop_llm（sub111 检测环）/ sub101 spawn_slow_writer_mock / sub139 capture_system_mock / sub303 spawn_mock303；`build_agents` 同步挂 full_auditor（+root 形参——e2e 契约面镜像生产装配；fallback 文本协议面由 src 单测覆盖，build_agents 原先配共享 JSON mock 即配对面破裂实锤：sub136 needs-review 假红暴露）。**回归**：e2e_write_next_contract **224/224 全绿**（含 5 个假阴性用例转绿：agent67 写链/ops72 守护写环/sub87 书会话写链/sub111×2 确认派发+检测环）。

## 五、模拟真人走查（修复后终态活体——重建二进制 + 新 walkthrough-env）

walkthrough-env rust 腿（ensure-rust-bin 增量重建后新环境）+ IAB 真实浏览器黑盒：

- **首页**：侧栏全导航树/写作数据 30 天（2466 字 2 章）/运行遥测累计 11 全成功/SSE ok 实时徽标/书籍列表镜花水月卡（2 章进行中）——无崩溃。
- **书详情**：面包屑/写作四按钮/模型徽章 Mock·lm-mock-model/上下文面板章节 01 镜中醒来 61+02 风起 2,405（真实字数）——706 号崩溃面回归守恒。
- **伏笔池**：4 条目（悬念身世/情感线/物品信物已撤回/世界观背景）+分类 chips；筛选「悬念」→chip 激活（violet）+列表收窄 1 条且 3s 足时复测稳定。备案：首轮点击后 800ms 内列表还原全量=面板内容重载重挂载竞态（数据重载一次后稳定），非产品缺陷。
- **写下一章（核心链路修复后活体）**：任务「**已完成**」+「已为 镜花水月 **完成**第 3 章《风起：林动》，字数 2405，状态 **ready-for-review**」+上下文面板章节 03 入列（2,405）。inkos.log：审计草稿→真相结算（8ms）无修复轮=JSON 契约 88 分通过（修复前同路径 50 分假阴性失败态已取证对照）。截图留档 `gui-test-screenshots/t987_write_next_ready_for_review.png`。
- **书籍设置页**：承诺账本 4 hooks（含 H03 已撤回徽标+撤回依据）/质量趋势**均分 88 已记录 3 章**（JSON 契约分值入 metrics 的用户可见面）/承诺紧迫度 H01/H02/H04/写法绑定空池提示（964 号池写入入口备案面）/新专名确认（苏檀）/张力曲线/三库资产种子——全渲染。
- **市场雷达**：扫描历史条目+免费扫榜 CTA 正常。
- **全程页面错误**：error/unhandledrejection 双钩跨 hash 导航收集——**0 错误**。
- 走查环境纪律：Playwright click 两次 actionability 卡住退化 DOM 级 `el.click()`（IAB 既有纪律）；收尾 pkill 特征限定 /tmp/inkos-walk-987+端口 8790/1240 清偿。

## 六、门禁终态

见 §二表。本号改动面=scripts/rcm（测试基建）+engine-rs 装配宏+engine e2e mock 配对面——gate:ts 十步终态复跑（rust-bin 0.2s 活体同源核验重建后二进制）+clippy 双 0+testgate 2113（+1）/584+audit 双 0+rcm 双腿 exit 0+U+FFFD 净，改动面完整验证。

## 七、教训

1. **契约序列型 mock 的消费序=产品审计消费者清单**——产品新增审计消费者（815 resync 审计段）时，所有注入递增序列的套件必须当轮复跑；「合并/特性大轮后契约套件未复跑=潜伏红温床」（706 教训）本轮第二次复发且一次两红。
2. **同实体多端口的缺省面必须在装配单点收敛**——「缺省回退最小协议」的缺省面躺在生产路径上=假阴性温床；815 号教训从 resync 单点扩面到装配宏（宏级统一=四调用点一次对齐，与 985 内联收敛同方法论）。
3. **契约 mock 与装配面是配对物**——改审计协议必须全量排查配对面（7 处 mock+build_agents）；共享 mock 服务多装配形态时，配对破裂静默假红（sub136 needs-review）。
4. **模拟真人走查是门禁矩阵的不可替代面**——UI 假阴性（50 分 audit-failed 失败态）在 gate:ts/testgate/rcm 三重门禁全绿下仅被真人链路走查暴露（用户可见的「失败」文案+质量趋势 50 分污染）；706 同型三重验证。

下一号自 988 起；出口回归常备；用户侧待办=Fork 推送（含 955-987）
