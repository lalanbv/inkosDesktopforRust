# 1424 号 · sweep 网络冒烟用例三发退避守卫轮——real_bootstrap 网络抖红清偿 + 1169 修复面全量验证

> 1423 出口执行。全量验证轮：全量门禁独立复跑（不信链上引用）+ 1169 修复面代码审查 + ignored-sweep 抓红归因修复 + 四站活体走查（本会话第一手模拟真人测试）。零 TS 代码改动；Rust 侧仅 1 枚 `#[ignore]` 测试 + sweep 预算一行。

## 一、全量门禁独立复跑（全绿）

- TS 三包：build + typecheck 全过；core 187 文件 1068 例 / cli 24 文件 91 例 / studio 165 文件 903 例全绿——903 与 1169 备案面（900+3）零漂移；测试文件数 165@1169 = 165@HEAD（备案「168 文件」为 vitest 汇总口径差，非删除）。
- audit:npm 独立跑全绿：5 advisory 全白名单拦 0 + 必红活体 4/4（与 1423 面一致）。
- cargo:testgate：engine-rs 52 目标 2114 passed + src-tauri 25 目标 593 passed 全绿；内联 audit:rust 腿双 crate 0 漏洞 + advisory-db 当日新鲜 + 必红活体 7/1；clippy:gate 双 0。

## 二、1169 修复面代码审查（新增代码逻辑全量审查）

- 四站守卫惯用形一致（`!nativeEvent.isComposing`）+ RecallTestCard `run()` busy 飞行互斥：`catch` 吞错误路径后 `setBusy(false)` 必达（无 finally 缺口）；React 离散事件逐次 flush，双 Enter 次发必读 fresh busy。
- 族扫完备性复核（三处「备案不扩面」站全部复核成立）：prompt-input 规范形自带 `isComposing || nativeEvent.isComposing` 双守卫且零消费方；ServiceDetail `handleAddModel` merge 去重幂等（`next.length === configuredModels.length` 早退）；ChapterReader 数字跳转 blur 双通道自中和。`keyCode===13` / 其余 Enter 形态全仓零漏站。
- 交互测试审查：`isComposing` 经 `KeyboardEventInit` 传 native 可达（字典合法键）；busy 双事件用例 `act` 分任务 flush 与 1162「同任务连发≠真实输入面」教训一致。

## 三、ignored-sweep 抓红归因与修复（本轮仅有的代码改动）

- 抓红：`--lib` 组 `engine::node::tests::real_bootstrap_downloads_and_verifies` 实测红——官方源 `https://nodejs.org/dist/v22.19.0/SHASUMS256.txt` connect 超时（30s）。
- 归因（逐流落盘证伪）：SHASUMS 安全设计**永远走官方源**（mirror 不可伪造，mod.rs 模块文档）→ 客户端 `connect_timeout=2s`（H3 审计预算）恰压在 CN 网络 TLS 握手波动带上（curl 实测 nodejs.org 1.3~2.5s 高方差 / npmmirror 0.38s 稳）→ 单发形态在本机网络下**必然偶发抖红**（988 绿 = 当日网络恰好通畅，非结构保证）。生产面语义不动：调用方回退系统 node 是刻意设计。
- 修复 = **用例级三发退避**（间隔 2s/4s；每发独立 tempdir——半途失败缓存残留绝不参与下一发判定，partial unpack 不得伪装 cache hit；持续不可达三发全败仍 panic，**地板卫兵必红语义不弱化**，持续网络故障照旧红报）+ sweep lib 组预算 300s→420s（三发失败路径合法最坏 ≈ 首发超时 30s + 退避 6s + 慢网全量下载 ~190s ≈ 230s，旧预算余量不足；真挂死仍远低于 420s，SIGKILL 哨兵语义不变）。
- 同族定性（修洞按族扫）：`real_engine_channel_check` 吞结果仅验不 panic——结构性免疫网络抖动，不入修复族；`test_is_newer_version_check` 纯逻辑（990 已勘误旧注「需网络」）。
- 验证链：单用例实跑绿（186.5s=慢网全量下载+疑似首发抖动重试面）→ **全 sweep 23/23 全绿清单精确匹配**（lib 组 67.4s 远低于新预算）→ clippy:gate 双 0 → `node --check` 语法核 → 两改动文件 U+FFFD 净。

## 四、四站活体走查复证（模拟真人测试第一手，本会话独立）

walkthrough-env rust 直启（8787）+ fixture（镜花水月）；靶向三证 = `?round=1424zhenhua` 本轮唯一 query 击穿缓存 + bundle 指纹 `index-BMggJ8PV.js`（**与 1169 备案同哈希零漂移**）+ 包内 `isComposing`×6；请求计数一律内核 `performance.getEntriesByType("resource")` 终审（1191 fetch-wrap 叠层假象教训），context-meter 5s 心跳显式归属排除。

| 站 | 组合回车（合成 `isComposing:true`） | 普通回车（真实 trusted key） |
|---|---|---|
| 1 ChatPage 聊天主输入 | **0** agent 请求 + 输入值保留 + handler 整体跳过 | **恰 1** POST /api/v1/agent + 用户消息渲染 + mock 回复流式 + 输入清空 |
| 2 Sidebar 改名 Dialog | **0** PUT + Dialog 保持不误关 + 值保留 | **恰 1** PUT /sessions/:id + 侧栏标题更新 + Dialog 关 |
| 3 RecallTestCard 召回测试 | **0** 请求 | **恰 1** POST /books/:id/hybrid-search + 结果渲染 |
| 4 AntiAi 经验沉淀 | **0** PUT | **恰 1** PUT /books/:id/experience + 「经验已沉淀」toast + 条目渲染 |

- 走查过程备案：书域「新建会话」按钮 hover 显露遮挡 → DOM 级 `el.click()` 惯用形（closest 选择器 Tailwind `group/book` 类名含 `/` 须属性选择器 `[class*="group/book"]`，裸传 closest 抛 SyntaxError）；空态「新建会话」点击未生效备案不深挖——改走首条消息自动建会话路径，反证 ChatPage 发送链活体；站 2 G2 后 1.5s 采样命中 Dialog 关闭帧边界（dialogOpen 瞬 true）——复查实时 DOM 已关，非缺陷。

## 五、清场备案

浏览器标签页关闭（remaining=[]）；env 进程组 SIGKILL（PGID 83421，含 mock+引擎子进程）；pgrep 零残留 + 8787/1234 端口释放 + /tmp/walk-1424-env.log 清除；/tmp/inkos-walk-env fixture root 按设计保留复用。

## 六、下一号自 1425 起

出口 = 下一巡检周期 / earendil-mz 单包合并（触发条件制已更新）；用户侧待办 = Fork 推送（含 955-1424）。
