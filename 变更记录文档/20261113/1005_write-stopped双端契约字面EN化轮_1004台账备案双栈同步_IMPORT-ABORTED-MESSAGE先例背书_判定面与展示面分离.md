# 1005 write-stopped 双端契约字面 EN 化轮——1004 台账备案的双栈同步轮

- **编号**：1005
- **日期**：2026-11-13
- **类型**：fix（双栈契约面 i18n——1004 号台账「write-stopped 双端契约字面（642 号，须双栈同步轮）」直系清偿）
- **范围**：`engine-rs`（1 文件 3 锚）+ `packages/core`（2 文件）+ `packages/studio`（3 文件）；六文件双 crate

## 立项

WRITE_STOPPED_MESSAGE 此前为 zh 字面（"写作已按您的要求停止。"），EN/ja 用户在协议面收到中文。1004 台账明确备案「改文需双栈同步轮，单侧改会破坏契约锚定测试」。本轮以 **IMPORT_ABORTED_MESSAGE 既存先例**背书方向（Rust 侧注释明文「英文锚文本；确认/聊天执行器据此本地化」）——错误协议面 EN 化、展示层本地化，与 1004 号 server.ts 409 判据同族。

## 关键定性：UI 展示与协议判定分离

BookDetail WriteStatusBanner 显示时不展示 lastError 原文——`isWriteStoppedMessage(lastError)` 命中即渲染词典键 `t("book.writeStopped")`（use-i18n.ts:240 双语早已就绪）。**协议面文案语言无关 UI 展示**，改造收缩为纯判定面 + 契约字面同步。

## 改造清单（锚定面普查三轮扩面）

1. **Rust 腿**（engine-rs/src/server/write_next_route.rs）：广播+快照 error 同源 user_message（:154）zh→EN；测试锚两处同步（:488 广播断言 / :510 快照断言）。注释同步 1005 备案。
2. **TS 判定面**（packages/studio/src/shared/write-stopped.ts）：WRITE_STOPPED_MESSAGE → EN；新增 LEGACY_ZH_WRITE_STOPPED_MESSAGE 旧形兼容常量（旧快照/旧广播数据仍含 zh 形）；isWriteStoppedMessage 认三形（EN 新形/zh 旧形/Operation aborted 遗留）。头注释 642 契约形态更新。
3. **core 通知面**（packages/core/src/utils/author-report.ts）：**普查扩面抓到的第三锚定面**——644 号噪音防线用全等判定 zh 字面，Rust EN 化后不再命中=「写章失败 [必须处理]」错误级噪音回归。扩判定：EN 新形全等 + zh 旧形全等（旧数据兼容）+ abort 长句 startsWith（TS 直连端点 server.ts:3383 广播原始 e.message 长句，原 `===` 全等本就漏——顺手收口）。
4. **测试面**：BookDetail.interaction 6 例（EN 主例+zh 兼容例+abort+真失败+writing+空态）；author-report 10 例（EN 主形态+zh 兼容+abort 长句前缀+遗留短字面+真实失败不受扰）；guard 台账 write-stopped 预算 1→0（LEGACY 常量经 zh-named 通道豁免，实况 0——台账同步判据自抓我方台账滞后，按设计工作）。

## NEUTER1005 必红活体

TS 判定面摘 EN 新形→BookDetail.interaction **恰红 1 例**（EN 主例独红，zh 兼容/abort/真失败/writing 例不受影响——分形语义自洽）→互逆恢复 6/6 复绿。Rust 侧 :488/:510 契约锚与 :154 字面同文件同步共处=锚定即活体（首轮 :510 漏网即红实证锚定有效）。

## 门禁终态

- Rust：write_next_route 4/4 绿；clippy --all-targets -D warnings 零告警；Rust src 内 zh 旧字面零残留
- core：typecheck 过（拦 startsWith 空安全一轮）+ build 过 + author-report 10/10 绿
- studio：typecheck 双 tsconfig 过 + 全套件 **872 例全绿**（871→+1，interaction 5→6）
- U+FFFD 扫描净；e2e_write_next_contract 不锚定文案（224 例无需复跑）；src-tauri 零锚定零改动

## 备案

- use-i18n.ts:240 book.writeStopped 键 zh 形保留（zh 展示文案正确性不随协议面 EN 化而变）。
- author-report 的 "aborted" 短字面全等（agent:error 引擎标记）保留原形态。
- TS 直连端点（server.ts write-next）中止广播仍为 abort 长句原文——两栈停止语义已由判定面统一，端点广播形态不动（794 号栈日志语义未触碰）。

## 教训

1. **锚定面普查必须用最小判别子串**——首轮 grep 完整短语漏掉 :510 无前缀断言（`contains("已按您的要求停止")`），测试红才现形；「按您的要求停止」四字子串才扫全。
2. **契约改文先查「判定面 vs 展示面」**——展示走词典键时协议字面语言无关，改造收缩为判定面同步，避免错误铺设 error-copy 映射。
3. **全等判定是跨端文案契约的隐性锚**——author-report 全等 zh 字面在普查「锚定面」时差点漏网（它在另一个包 core！跨包锚定面普查必须做）。
4. **台账同步判据防自己**——guard 预算=实况判据抓到本轮台账更新滞后（LEGACY 常量 zh-named 豁免后实况 0），守卫按设计约束作者本人。
5. **vitest 不查类型**第二次实证（core startsWith 空安全由 typecheck 拦截，测试先绿）。
