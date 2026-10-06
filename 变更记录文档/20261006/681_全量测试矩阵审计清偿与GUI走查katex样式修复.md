# 681 号：全量测试矩阵复验 + audit:npm 三告警 override 清偿 + GUI 走查 katex 样式缺失修复

日期：2026-10-06。类型：fix(deps)+fix(studio)+docs。前置：680 号（HEAD 26284af8）之后用户发起「全量测试、代码审查、模拟真人测试、确保功能正常无 BUG」专项。

## 一、全量测试矩阵（680 号 HEAD 基线，全部绿）

- `cargo:testgate`：RustSec 审计双 crate 0 漏洞 + engine-rs 45 目标 1926 passed（INKOS_DUEL=1 真跑）+ src-tauri 25 目标 584 passed——与 678/680 基线同数零漂移。
- `clippy:gate` 双 crate 0 告警；`gate:ts` 八步全绿（build/typecheck/test/audit:npm/rust-bin/node-fallback-smoke/engine-contract-diff/export-epub-smoke）；`review-matrix` 双腿 7/7×2（各 50 断言）；`bench:gate` 通过（最大 +15.4% 带内，阈值 +30%）；`verify:engine-bindings` 187 导出绿；`verify:publish-manifests` 3 包 OK；`audit:semantic-patterns` 0。
- studio Playwright e2e 36/36（2.2 分钟，LLM stub 模式）。

## 二、audit:npm 三告警清偿（pnpm-workspace.yaml + pnpm-lock.yaml）

678 号终锚定后新披露 advisory 三连，gate:ts `audit:npm` 步拦截（白名单纪律：新告警不静默绿灯，修复优先于白名单）：

| 模块 | 级别 | 公告 | 链路 | 清偿 |
|---|---|---|---|---|
| proxy-addr | critical | GHSA-jqcg-44mw-7w3h（IPv4-mapped IPv6 信任子网 IP 欺骗） | express←MCP SDK←@google/genai←pi-ai←**inkos-core 运行时** | `"proxy-addr@2": ^2.0.8` |
| source-map-js | high | GHSA-68fv-2mgg-jv7q（索引化 section 偏移事件循环 DoS） | vite/postcss/jsdom（纯 dev 工具链） | `"source-map-js@1": ^1.2.2` |
| katex | low | GHSA-238p-pmpm-9mq7（原型污染绕过 trust 限制） | @streamdown/math + mermaid（studio 渲染运行时） | `"katex@0.16": 0.18.2` |

- katex 0.16→0.18.2 为跨 minor override（生态 @streamdown/math 1.0.3 仍钉 ^0.16，无补丁线可用）；katex CSS 由 @streamdown/math 从同一包 import（`katex/dist/katex.min.css`）故 JS/CSS 自动同版无错位；渲染面经 GUI 走查活体验证（见三）。
- 范围键按 556 号教训（精确键只覆盖命中版本，新解析版本逃逸）。
- 附带钉 `"caniuse-lite": 1.0.30001809`：npmmirror 对 1.0.30001810 的 CDN 404（packument 列出但 tarball 缺失，镜像同步缺陷）导致锁文件重解析失败；1809 满足 browserslist@4.28.8 的 ^1.0.30001809 下限（429 号 browserslist 数据钉版先例）。
- 清偿后 `audit:npm` ✓ 0 advisory；gate:ts 八步复跑全绿零回归。

## 三、GUI 黑盒走查（模拟真人）与 katex 样式缺失修复（packages/studio）

**方法**：真实服务链（studio API :4569 + vite :4567 + scratch 项目 /tmp/inkos-gui-walkthrough）+ 浏览器黑盒（真实点击/键盘，DOM 快照+截图双验证；截图存 gui-test-screenshots/，gitignore 不入库）。环境预置=scratch 项目夹具（book.json/story_frame.md/章节文件），不替代任何被测行为。

**测试点**：T1 首页加载（SSE 实时绿点/书籍卡片/写作数据统计）✓；T2 对话式建书页（空输入禁发 ✓；agent 提交流需 LLM key=657 号已知挂起，e2e stub 已覆盖）✓；T3 数学公式渲染；T4 设定抽屉（无 math/mermaid 插件=现状设计，原文+代码块展示）✓；T5 章节阅读器（4 段正文排版正常）✓；T6 命令面板 ⌘K（过滤+跳转项目设置）✓。

**发现 1（已修复）**：KaTeX 渲染样式从未被引入——`@streamdown/math` 的 `getStyles()` 约定宿主引入 `katex/dist/katex.min.css`，但 streamdown 包不消费 getStyles（dist 0 次引用）、studio 源码 0 处 import → 全页样式表 0 个 katex 条目、KaTeX 字体 0 加载、`.katex-mathml`（MathML 辅助文本）无隐藏规则视觉泄漏。**既有 BUG 与 katex 版本无关**（升级前后都不会有 CSS），36 个 e2e 无数学渲染断言故从未暴露。修复两处：`src/index.css` 增 `@import "katex/dist/katex.min.css"`；`packages/studio/package.json` 增直接依赖 `katex: 0.18.2`（裸包解析需直接依赖，pnpm 严格隔离下传递依赖不可达）。修复后活体复验：公式 LaTeX 排版正确（∫₀^∞ e^{-x²}dx=√π/2）、KaTeX 字体 3 个加载、MathML 泄漏消失。
- 附带定性：单 `$` 行内公式不渲染=插件默认（`singleDollarTextMath: false`），`$$...$$` 才渲染——设计行为非缺陷，备案。

**发现 2（备案未修，P3 边界缺口）**：章节文件名前后端约定不一致——studio 详情路由 `chapters/:num` 按 `padStart(4,"0")` 前缀找文件，core `rebuildChapterIndexFromFilesAt` 用 `^(\d+)[_-]?` 宽松匹配任意位数 → 非 4 位命名章节（`001_x.md`）「能列出、打不开」（404 Chapter not found）。引擎双端写章节恒 4 位补零（write_next.rs:340 `{:04}`、runner.ts:1250）故**主路径不受影响**；暴露面=手工放置/导入的非标命名文件。建议后续号统一两侧约定（详情侧 fallback 宽松匹配或索引侧收紧 4 位），涉语义决策不擅改。

**其他观察**：IAB 后端 Playwright locator click 在该 SPA 高频重渲染下 3s 预算内超时（cua 坐标点击正常）——测试工具适配观察，非产品缺陷。

## 四、代码审查（680 号增量）

`scripts/review-cycle-matrix.mjs`（+70/-16）：engine 旗标校验（非法值 exit 2）、rust 腿新鲜度闸（build-failed/missing 拒绝出具证据）、断言面按引擎分流（rust=inkos.log/node=env.log，日志面缺失即断言失败不静默）、`[env] ✗` 早退、头注释七场景纠偏——逻辑面无缺陷。一个 P3 建议：闸门对 `uncertified`（cargo 不可用但旧二进制在）与 `skipped`（显式 env）状态放行，严格证据套件可考虑一并拒绝（ensure-rust-bin.mjs 状态族核验；改语义另号立项）。

## 五、教训

1. **传递依赖的「宿主引入」约定（getStyles 模式）必须有一次活体渲染验证兜底**——约定断裂（streamdown 不消费+studio 未引入）在「测试全绿」下潜伏：门禁覆盖不到的最后一公里是「浏览器里它真的渲染对了吗」。
2. **升级类 override 必须配渲染面活体验证**（katex 0.16→0.18 跨 minor）——本号 GUI 走查同时完成了 override 验证与既有 BUG 发现，一举两得。
3. **e2e 断言面盲区=「无断言的功能等于不存在的功能」**：36 spec 无数学/mermaid 渲染断言，样式缺失潜伏至今；后续可为 SummarySection 补 katex 渲染 smoke（另号）。
4. 镜像同步缺陷（packument 与 CDN 不一致）的应对=钉镜像可用线+注解备案，不引入 npmjs 混源（锁文件 registry 纯净性）。
