# 680 号：review-matrix 补 Rust 腿——审改循环七场景双腿可复跑护栏（673 备案套件化清偿）

日期：2026-10-06。类型：feat(scripts+package.json)。前置：659–669 号审改循环矩阵七场景全部硬编码 `--engine node`；673 号零代码活体只覆盖 rust 腿 patch-only 主路径+达标退出，其备案明文「Rust 腿的未产出/未净提升/回退分支未逐一注入……如需逐一镜像注入另号」；674b 号手工补了未产出/未净提升/达标退出三分支（一次性活体）。**净提升多轮（D）/三轮链（G）/多轮未产出（G2）/restore 回退（E）四分支在 Rust 管线从未活体过，且全部历史证据均为一次性手工注入，无可复跑护栏。**

## 一、实现（scripts/review-cycle-matrix.mjs + package.json，零 TS/Rust 源码触碰）

1. `--engine rust|node` 旗标（缺省 node=`pnpm review-matrix` 语义零变化）；新增 `pnpm review-matrix:rust` 入口。
2. rust 腿前置 `ensure-rust-bin` 新鲜度闸（复用 643 号模块，641 教训「testgate 不编译 bin 目标」结构性清偿的套件内化）：build-failed/missing 即拒绝出具活体证据；本跑 `fresh`（cargo 指纹核验）。
3. 断言面按引擎分流：node 腿审改循环日志走 stdout（env.log）；rust 腿走 `on_log→inkos.log`（674b 管道，`{timestamp,level,tag,message}` JSON 行、serde_json 不转义中文=关键词逐字可匹配；服务端管道全级别落盘无过滤——books_routes with_event_broadcasts 实证）。promises timeline 断言恒走 env.log（fixture stdout，引擎无关）。落盘痕迹断言（chapters/*.md）双腿同表。
4. 就绪等待补 `[env] ✗` 早退（walkthrough-env 二进制缺失/mock 未就绪等硬失败不再空等 300s）。
5. 头注释同步：659 号「四场景/未产出不入套件」陈旧段纠偏（665 号同类教训）——七场景全量入册；Rust 侧日志关键词与 TS 逐字同形（chapter_review_cycle.rs 五条 zh 文案 grep 实证）故场景表双腿共用零分叉。

## 二、红绿（双腿各 7/7，50 断言/腿）

- **node 腿回归**：`--base-port 8837` 7/7 绿（42/37/42/42/42/47/42s）——改动对既有行为零回归。
- **rust 腿首跑**：`--engine rust --base-port 8857` 7/7 绿（37/32/37/37/37/42/37s）——**D 双 PATCH/G 三轮链/G2 多轮未产出/E restore 回退为 Rust 管线首次活体，全部首跑即绿**；B/C/F 与 673/674b 手工证据对齐。
- 抽验排除空转绿：G 场景 inkos.log 三轮修复行（45/55/70）逐字落盘+落盘正文双 PATCH 痕迹各 ×1（缓缓收紧五指/掌纹里渗出细密的汗意）；E 场景回退行逐字（「回退到最高分版本（45 分 vs 当前 91 分）」）+落盘正文替换句 ×0（超长修订确被回退落初稿）。
- mock 预检定性：REVISE_BLOAT 拼接点=PATCH 替换文本（非 REVISED_CONTENT），patch-only 形态下 restore 分支双腿等价可触发（walkthrough-mock.mjs:274）；`writing.reviewRetries` 双端消费实证（write_next.rs:158→:938）——G/G2 的 retries=3/2 注入 rust 腿有效。

## 三、证据形态备案

- rust 腿首跑即绿（非红→绿）：断言实质含分支专属日志行+落盘工件痕迹，空转绿不可能（管道未执行对应分支则关键词与 PATCH 痕迹均不存在）；且 673/674b 已先证 mock 契约（审稿 JSON/PATCHES 形态）被 Rust 解析器活体消费。但「首次覆盖即绿」的证明力弱于红→绿，如实备案。
- 门禁裁剪备案（535 先例）：本轮零 TS/Rust 源码触碰（scripts/*.mjs+package.json scripts 段），clippy/testgate/gate:ts 不重跑——双腿套件全绿即本轮改动面的完整验证；node --check+JSON.parse 语法面过。
- G2 场景 rust 腿断言「落盘含 PATCH1 痕迹、不含 PATCH2 痕迹」依赖 Rust 侧未产出判全等语义与 TS 同构（674b 已证单轮形态）；多轮（FROM=2）形态本轮首证。

## 四、教训

1. **一次性活体证据不是护栏**：673/674b 的手工注入再真实也只证明「当时那一次」——备案句「如需逐一镜像注入另号」挂了两轮才清偿；验证基建（套件）与验证行为（跑一次）是两种产物，后者不可引用为前者。
2. **断言面分流要跟着日志出口走**：同一行为双端日志通道天然不同（TS config.logger→stdout vs Rust on_log→inkos.log，674b 定案），套件双腿共用场景表的前提是断言面显式按引擎选择，而不是假设日志面同构。
3. **套件头注释随扩容同步**是 665 号教训的第三次复发点（659 头注释在 667/669 两次扩容后仍写「四场景」）——扩容 PR 的 diff 里应包含头注释场景清单行。
