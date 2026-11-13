# 1006 provider 错误诊断双栈 EN 基准化轮——cli/core 族扫收官 + 跨端解析契约标记双形兼容

- **编号**：1006
- **日期**：2026-11-14
- **类型**：fix（i18n 专项续作——1004 号「TS 三包普查」收官轮 + provider 诊断块双栈 EN 基准化）
- **范围**：`packages/cli`（零改动，负结果定性）+ `packages/core`（provider.ts）+ `packages/studio`（error-copy/server.ts/guard/新测试）+ `engine-rs`（service_routes.rs）；双栈五文件

## 一、cli 族扫（负结果亦是结果）

初扫 47 处硬编码候选，细读定性**全部合规、零真缺陷**：

1. **39 处 = lang-early-return 形态**（普查器第三形态盲区）：`if (locale === "en") { return EN形 }` 早退 + 尾部 zh 默认分支——if 与 CJK 所在 return 是**兄弟节点**，不在祖先链上，lang-if 判据天然抓不到（effects.ts buildStyledHelpSections/buildHelpFooter/localizeThemeLabel、setup.ts buildInteractiveSetupCopy 等五函数）。TUI 已有完备 i18n 机制（tui/i18n.ts TuiCopy 双语表 ZH_CN/EN + resolveTuiLocale + getTuiCopy），这批是机制内合规实现。
2. **6 处 = 中文斜杠命令解析器**（local-commands.ts `/深度 浅|轻量|标准|普通|深|深入`）：zh 用户的输入别名面（正则+switch 解析），非展示文案。
3. **2 处 = LLM schema description 中文示例值**（forecast/short-fiction `e.g. 女频短篇…`）：给 LLM 的示例非 UI 文案。

普查器补 lang-early-return 判据后 cli 复扫归零。**修复零行、判据产出一条**。

## 二、core 普查定性（570 候选）

判据切换：core 是引擎库，CJK 判据=「与创作语言无关的用户直显文案」。定性结果：

- **豁免（引擎/内容/契约面）**：LLM prompt（script-storyboard 90 处/architect/writer 等——创作语言内嵌=产品核心设计，2.0 双栈 golden 锁定改文即破坏契约）、hook-kind zh 类型名注册表（698/976 号 Rust 镜像+golden）、schema description 中文示例（agent-tools 7 处）、chapter-splitter 内容解析、provider endpoints 专名、cover 内容组装。
- **真缺陷面 = provider.ts 六条 HTTP 诊断块**（400/401/403/429/5xx/connection，25+ 文案）：终端 CLI 与 chat UI 直显，与创作语言无关。console.warn 3 处（开发者日志）与 `includes("不支持")` 2 处（上游 zh 错误文本解析判定）豁免。

## 三、修复（双栈五文件）

1. **provider.ts**：六块 EN 基准化（对齐 effective-llm-config.ts:117 EN 消息先例与 1004/1005 号错误协议全 EN 族判据）；「上游详情：」标记转 **「Upstream details: 」**。
2. **error-copy.ts**：新增 8 条整块正则映射（400/5xx 各分有/无 detail 两型；动态段捕获组保留）承接 zh 展示——zh 用户行为不变，EN/ja 用户首次看到可读诊断。台账预算 3→11。
3. **双栈解析点两形兼容**：studio server.ts:2164 与 engine-rs service_routes.rs:1107 的 `contains("上游详情：")` 均扩 `contains("Upstream details: ") || contains("上游详情：")`（旧快照/旧日志数据兼容）；agent_production.rs:788 分类正则 `(?i)…upstream…` 已天然兼容 EN 新形免改。
4. **Rust 夹具**：service_routes 新增 `upstream_detail_marker_dual_form` 测试（EN 主形态+zh 旧形态双通道验证）。

## 四、测试与红绿闭环

- **error-copy.test.ts 新建 9 例**：六块逐块以生产同形 EN 样例喂入断言 zh 关键片段（映射正则与生产字面漂移即红——多一个标点即断）+ EN 模式不映射 + 未知消息原样 + 409（1004 号）锚定。**首轮恰红 1 例（5xx 有-detail）实证活体有效**——修复轮次中重复条目清理时误删了有-detail 版，测试抓到（映射正则正确性活体=生产漂移防线）。
- **NEUTER1006**：摘 403 映射 → error-copy.test 恰红 1 例（403 独红，其余 8 例不受影响——分形自洽）→ 互逆恢复 12/12 复绿。
- **guard 台账同步判据双次自抓**：error-copy 预算 3→10→11 两轮漂移均由「预算=实况」判据自动拦截（含修复轮次内部临时态），守卫约束作者本人按设计工作。

## 五、门禁终态

- core：typecheck 过 + build 过 + **1062 例全绿**（provider 文案零锚定实证）
- engine-rs：service_routes 15/15 + upstream_detail_marker_dual_form 1/1
- studio：typecheck 双 tsconfig 过 + **881 例全绿**（872→+9）
- U+FFFD 净；cli 普查归零；src-tauri 未触碰（备案 1007）

## 备案

- **src-tauri 桌面壳 165 处 CJK**（error.rs with_suggestion 族等）：无语言机制（AppError 建议文案 zh 硬编码），需先设计桌面壳语言机制（INKOS_LOCALE 先例可复用）——**1007 专项候选**。
- provider console.warn 3 处（开发者日志）与 `includes("不支持")` 解析判定 2 处维持豁免（1004 判据）。
- cli普查器判据（lang-early-return）未入守卫测试——studio guard 八夹具已覆盖主要形态，cli 面真缺陷=0 暂无守卫必要；若 cli 后续新增面可按 1004 形态立 cli 守卫。

## 教训

1. **普查器形态盲区按「兄弟节点」找**——if-early-return 语言分支的判据缺口源于「条件上下文爬链只看祖先」，兄弟 if 永远漏；函数级早退扫描是第三形态（三元→if 链→早退块）。
2. **跨端解析标记是隐性契约**——「上游详情：」不只是展示文案，还是 TS→Rust 错误文本的解析锚（两处 contains），改文必须双栈同步且保留旧形兼容；锚定面普查要按「谁解析这段文本」追踪，非只按「谁断言这段文本」。
3. **映射正则的活体=生产同形样例**——手写样例若与生产字面漂移（标点/空格）映射静默失效；error-copy.test 首轮恰红实证其防线价值（修复过程中误删映射即红）。
4. **负结果轮同样产出判据**——cli 47 处零缺陷但产出 lang-early-return 判据与「TUI 机制已就位」的定性，为后续 cli 新增面建立基线。
