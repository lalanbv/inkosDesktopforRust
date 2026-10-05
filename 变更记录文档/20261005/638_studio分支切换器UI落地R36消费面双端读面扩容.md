# 638 号：studio 分支切换器 UI 落地——R36 会话树化消费面双端读面扩容

日期：2026-10-05
提交：本次（develop）
前置：637 号（3aaa0082，transcript 树化双端 + POST /sessions/:id/branch + head/branchCount 透出）

## ①缘起与路线

637 号把 R36 会话树化的引擎能力全部落地（parentSeq 链语义 + branch_moved + head replay + 双端 REST 分支写面），但当时备案「studio 分支切换器 UI 另立项」——引擎有分支能力而无任何用户可见入口，等于功能不存在（508b 先例：端点要有首个消费方）。本号即该消费面：让「从第 N 轮重开但弃用路径可回看」的创作分支体验在桌面 UI 成立。pi 1.0 key 第六轮未复测（无新前提，缓行维持不变）。

## ②设计与实现（三层）

1. **Core 分支点读面**（`packages/core/src/interaction/book-session-store.ts` 新增 `deriveSessionBranchPoints`）：一个可回退分支点 = 一条 `request_committed` 事件。语义依据：每轮事件序为 request_started → 消息 → request_committed（消息先于 commit 落盘），故 commit seq 恰为「该轮完整结束」的分支目标（637 号 smoke 探针 branch 到首轮 commit 后 derive 双轮文本俱全即此语义的活体证明）；branch 到轮中任意点则孤儿化该轮（未 committed 的 request 分支自然剪除，637 号已锁语义）。preview 取同 requestId 的 `request_started.input`——每轮必有、纯文本，免解析 pi message 载荷形状（设计裁决：不用 message 事件内容，少一个 content 形态依赖）。`onActiveChain` = 该提交点是否在当前 head 链上（false = 已弃用分支，可切回）。失败轮（request_failed 无 committed）不是分支点。会话不存在返回 null → 服务端 404。
2. **双端 REST 读面**：TS `server.ts` 与 Rust `session_routes.rs` 各加 `GET /api/v1/sessions/:sessionId/branches` → `{ sessionId, head, branchCount, points[] }`（点：seq/requestId/timestamp/preview/onActiveChain，camelCase 对齐）。Rust `derive_branch_points` 镜像（`SessionBranchPoint/SessionBranchPointsResult` serde rename）。
3. **studio 消费面**：
   - store（`slices/message/action.ts`）：`branchSession(sessionId, toSeq)` = POST branch → 成功后 `loadSessionDetail` 重拉（active 链投影立即生效）；409 忙/400 目标非法按既有 fetchJson 管道抛 Error(server message)（与「重试」按钮显示服务端消息同先例）。
   - 组件（`components/chat/SessionBranchSwitcher.tsx`）：挂聊天输入状态条（ContextMeterBadge 旁，ChatPage 底栏）。点开拉分支点清单（seq 降序 = 最近轮在前），每点渲染「第 N 轮 + 当前/已弃用分支徽章 + 相对时间 + 预览截断」；「当前」= 在链点位 seq 最大者（head 至少推进到它）；点击 → ConfirmDialog（明示「之后的轮次不会删除，可随时切回」——非破坏语义写进用户文案）→ branchSession → 面板关闭。draft 会话不渲染（无文件无分支点）；流式中禁用（服务端亦有 409 双保险）。

## ③红绿可证伪

- Core：`session-branch-points.test.ts` 5 断言（线性两轮全在链 + 分支后旧链点 off-chain/新链点 on-chain/branchCount=1 + 失败轮不产点 + 孤儿 committed preview 空兜底/会话缺失 null + resetLeaf 后链空 head=null 全 off-chain）——stash 实现红 5/5 → 恢复绿 5/5。
- Rust：`book_session_store.rs` 测试模块 +6 镜像同五态 + JSON camelCase 形态（requestId/onActiveChain 键存在性），15/15 绿。
- UI：`SessionBranchSwitcher.interaction.test.tsx` 5 断言（jsdom+testing-library，双 mock 惯例：fetchJson 按路径路由 + store 模块选择器桩）——无 sessionId/draft 不渲染、面板点位渲染全要素（轮号倒序/当前徽章/弃用标记/JS 预览截断 42 字阈值/相对时间/计数徽标）、确认弹窗载荷 `branchSession("s1", 4)`、409 错误消息留面板可重试、拉取失败/空点位两态。组件 stash 红 → 恢复绿 5/5。
- 差分器：branch 维度内新增分支点读面对照（双腿 GET /branches：形态锁死——点位数 ≥2、preview 字符串/onActiveChain 布尔；结构面硬比 branchCount+points{preview,onActiveChain}；seq/timestamp/head 绝对序数豁免备案同 637 head 豁免因果链）。对照 61 端点 0 分歧。

## ④插曲（门禁抓伪象两例）

1. **差分器新维度首跑 ✗ rust=null**：node 返回完整 payload 而 rust 腿空——嫌疑排序第一位就是陈旧 debug 二进制（差分器起 `target/debug/inkos-engine-server`，新路由未编入）。`cargo build --bin inkos-engine-server` 增量重编 18.9s 后复跑绿。**632/636 号「先重建再判读」教训第五次实证**——本号新证据：判据可前移到「新加的端点/维度首跑分歧时，先看二进制新旧再查代码」。
2. **测试先行抓 fixture 伪象一例**：交互测试长预览断言初版 fixture 只有 36 字（< 42 字 JS 截断阈值），断言 `…` 失败暴露「截断由 JS 预览层做、CSS truncate 只管视觉溢出」两层截断的事实——fixture 加长到 54 字后断言真实覆盖 JS 层。行内注释写在对象字面量属性值后吞逗号造成一次语法错（node 层 esbuild 报错直指行号，现场修复；与 634 号插曲同类：块注释插入位置敏感性）。

## ⑤门禁矩阵（本号 HEAD 全量实跑）

gate:ts 七步全绿（build 16.5s/typecheck 16.0s/test 74.8s/audit:npm 2.0s/smoke 34.1s/差分 40.1s 61 端点 0 分歧含新维度/epub 34.1s）+ clippy 双 0（--all-targets，clippy-gate 脚本）+ cargo:testgate engine 45 目标 **1921 passed(+6)** + src-tauri 584 + audit:rust 双 0（testgate 首步）+ duel 真跑 10/10（INKOS_DUEL=1，41.4s）+ bench:gate 首跑负载闸 39%/核拦截照 529/631/637 先例候谷复跑（2.28/18=12.7%/核）通过零回退。smoke 双腿在 gate:ts 内绿（branch 探针 16/16 仍覆盖写面；branches 读面由差分器双腿永久对照——探测面分工备案）。

## ⑥备案

- 分支点 UI 只做「已提交轮」目标集，resetLeaf（toSeq=null）不设 UI 入口——真「全部重来」的等价动作是新建会话，resetLeaf 保留为 API 面。
- `SessionBranchPointsResponse` 类型落在 studio store types（消费侧定义），core 不再向 studio 出第二份类型——studio 与 core 类型重复面已由 491 号审计定性为可接受（跨包引用成本 > 字面重复）。
- 轮号显示为 seq 序倒序序数（第 N 轮按点位数倒排），非全局历史轮号——分支后「第 2 轮」可能对应不同 requestId，这是树形历史的本性，预览文本承担消歧。

## ⑦教训

1. **引擎能力落地与首个消费方同程才闭环**：637 备案 UI 另立项是对的（写面语义先稳），但消费面一日不落地，双端 REST + 树化语义就一直在「无用户可见性」状态漂移——读面（GET /branches）在消费方立项时才被逼出来补齐，说明 REST 最小暴露的「最小」要以首个消费方的真实需求为准线复核。
2. **新端点首跑分歧的第一嫌疑是二进制新旧**（先重建再判读第五实例）：判据固化——凡差分器/smoke 起本地 debug 二进制的门禁，改了 Rust 路由后必须先 `cargo build` 再跑门禁，此序应进肌肉记忆而非靠撞墙回忆。
3. **读面派生字段选「必然存在的平凡源」**：preview 取 request_started.input 而非解析 message 载荷，一个字段少一类形态依赖——跨端派生字段的健壮性在设计期由数据源选择决定，不由解析 defensive 程度决定。
4. **JS 截断与 CSS 截断是两层**：测截断行为必须让 fixture 真的越过 JS 阈值，否则断言打在 CSS 视觉层（jsdom 不可测）上空转。
