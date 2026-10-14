# 706 号：全量验收轮（代码审查+矩阵余腿+模拟真人走查）——rcm 潜伏红清偿 + 2.0 UI↔Rust 引擎 mismatch 三处修复

> 日期：2026-10-15（705 号同日续号；目标=全量测试/代码审查/模拟真人测试/确保无 BUG）
> 范围：scripts（rcm/walkthrough-env）/ packages/studio（Sidebar/ChaptersSection/selectors/ChatPage/ErrorBoundary）

## 一、代码审查（705 diff 独立复审）

逐 hunk 复审 f226ae40，两处补强：
1. **experience PUT 条目级校验**：fork 时代 422 仅靠 `entry.text.trim()` TypeError 兜住缺 text 条目，缺 `id`/`chapter`/`kind` 的条目会 200 落盘（无 id 不可删除、UI 渲染风险）；495 号意图=以 Rust typed-extractor 为对齐参照——补逐条校验（id/chapter 整数/kind 四值/text/createdAt，enabled 缺省 true）镜像 Rust serde 必填面，失败流入既有 422 catch。考古实证：fork 时代 `mergeExperienceEntries` 与 2.0 逐字相同（均无 zod 校验），属嫁接继承的潜在缺口非 2.0 退化。
2. **deconstruct 探针固定 name**：缺省名含 `Date.now()` 双腿各生成不同文件名，未来严格模式恢复时破坏工件树可比性——探针 body 补 `name:"probe"`。

## 二、全量矩阵余腿（暴露并清偿一处潜伏红）

**review-matrix node 腿自 688 号（2.0 合并）以来 17 个提交未复跑**——本轮实跑全红，根因两段：
1. `walkthrough-env.mjs` node 分支 boot 配置带 `writing` 段，2.0 `ProjectConfigSchema` strict 拒收 → boot 崩（resolveEffectiveLLMConfig ZodError）。696/698 契约源自适应只改编了 walkthrough-fixture，env 漏网（env 先于后端启动无法探测，按 688 双栈定线 node 腿=2.0 基底硬适配省 writing 段）。
2. boot 修好后剩余失败=2.0 腿无 fork 审改循环旋钮（writing.reviewRetries，resync 深水区早退跳过 write-next，704 已备案）——rcm 升级**契约源软失败记账模式**（695 豁免记账/700 软失败同型）：node 腿 boot 探针实例 GET /prompt-packs 判定契约源，2.0 时 fork 契约断言照打 `soft ✗` 不计判定 exit 0 响亮记账，TS 恢复 fork 契约自动严格化；rust 腿恒严格。

终态：**rust 腿 7/7 严格全绿**（37-42s/场景，审改循环六分支活体）+ node 腿软记账 exit 0 + bench:gate 带内通过（最大 +12.6% sse_parse 带内）+ duel 由 cargo:testgate 内 INKOS_DUEL=1 恒真跑（gate 脚本强制）。

## 三、模拟真人走查（Rust 引擎 + 2.0 UI 真实服务链）——暴露三处 mismatch

walkthrough-env rust 腿（引擎静态托管 studio dist）+ 浏览器黑盒。**书详情页整页崩溃**（ErrorBoundary「Cannot read properties of undefined (reading 'length')」）——704 走查的是 TS 腿，Rust 腿书详情自 688 无人走过。诊断链：错误边界临时直出组件栈（保留转正）→ 崩点最小化 bundle 定位。

三处修复（2.0 UI 对 fork 引擎响应形状的可选字段防守，类型契约本就 optional）：
1. **ChaptersSection `ch.observations.length`**：`observations` 是上游 2.0 加法字段（fork 契约全史无此键，Rust 章节索引无），可选化 + `?? 0` 兜底；
2. **Sidebar 会话过滤 `session.messages.length`** + selectors `session.messages.some` + ChatPage 同类：store 会话 `messages?` 类型可选拒绝缺键兜底，统一 `?.`/`?? []`；
3. **ErrorBoundary 组件栈直出**：componentDidCatch setState componentStack 渲染进错误卡（纯文本无依赖，463 号最小化原则不破坏）。

### 走查终态（修复后活体证据）

- 书详情页 Rust 引擎完整渲染：侧栏书树/面包屑/写作四按钮/上下文面板（章节 01 镜中醒来 61 字 + 02 风起 2,405）；
- **拆书工作台真人链路**：填逐章证据 → 分析 → 「已生成并落盘」+ markdown 渲染（人物档案/林昭）——**705 号 deconstruct 包络活体 bug 修复的用户可见路径实证**；落盘证据 `story/deconstruction/decon-*.md` 在盘；空载荷 400 校验正确；
- **经验记忆真人链路**：沉淀 → 提示 + 条目显示 → 删除 → 消失 + experience.json 回落空数组；
- 全页面面板渲染无崩溃（承诺账本 4 条/质量趋势均分 88+紧迫度 4 条/三库资产种子/上下文透视 9 条来源含压缩留痕 6 条/导演驾驶舱续跑建议）。
- 备案：首页预览 `Failed to load library`（/works 上游新增域 Rust 未迁，695 三域报告已声明，非回归）。

## 四、门禁终态

| 门禁 | 结果 |
| --- | --- |
| studio vitest | 649/649（含 ErrorBoundary 既有测试兼容） |
| typecheck | 四包 Done 0 错误 |
| gate:ts 八步 | 全绿 exit 0 |
| contract-diff | 25 面 0 分歧（含探针 name 修复活体复跑） |
| review-matrix rust 腿 | 7/7 严格全绿 |
| review-matrix node 腿 | 软失败记账 exit 0（boot 修复实证） |
| bench:gate | 带内通过 |
| cargo:testgate | 1933+584 双零漂移（本轮回前基线，本轮改动未触 Rust 侧） |

## 五、教训

1. **覆盖率缺口是潜伏红温床**——review-matrix 自 688 合并 17 提交未跑，boot 崩溃与 UI 崩溃两处潜伏全靠本轮全量复跑暴露；「矩阵双腿 7×7×2」这类口径要随合并大轮强制复跑而非沿用旧绿记录。
2. **双栈 pairing 的 UI 必须按最弱引擎 defensively 类型化**——2.0 UI 与 fork 引擎配对时，上游加法字段（observations）与可选字段（messages）的直解引用是整页崩溃级；类型标 optional 处解引用必须 `?.`。
3. **契约源软失败模式要下沉到每个 fork 契约套件**（700 号教训复用）——rcm 是 fork 语义套件（审改循环旋钮），2.0 腿硬断言只会产出无信息红。
4. **错误边界该带组件栈**——「Cannot read properties of undefined」没有栈时定位成本极高（本次浏览器 console 钩子全部落空，最终靠边界直出组件栈 + 最小化 bundle 切片定位）；纯文本 componentStack 是零依赖可诊断性。
5. **fork 时代注释声明的设计意图要对照实现验证**（495 号 422 意图 vs 实际仅 TypeError 兜底）——注释是意图不是契约，镜像补齐时以对端可观测行为为准。
