# 1004 全站硬编码 CJK 普查修复轮——AST 级分类器定性 + 40 处用户可见面清偿 + 台账预算守卫

- **编号**：1004
- **日期**：2026-11-13
- **类型**：fix（i18n 专项——1003 号备案「全站 zh 硬编码 i18n 面未普查如需另立专项」直系清偿）
- **范围**：`packages/studio`（16 文件修复 + 1 守卫测试新增）；Rust 树零改动

## 立项与普查方法

1003 备案转正。自建 AST 级普查器（TypeScript compiler API，逐节点定性非 grep——720 号「单行信号扫描假阳性」教训的对偶应用），对 studio src 全量 `t/tr` 之外的 CJK 字符串字面量/模板段/JSX 文本分类：

- **合规通道识别（7 类既存惯用形）**：`tr("中文","English")` 内联双语（app-language.ts，668 处）、`["中文","English"]` 元组对（parts-builder 工具名映射形）、调用实参双语对（server.ts `pick(lang,zh,en)`/commands.ts `nav(id,zh,en,…)` 文件内 helper）、兄弟 `en:` 属性、双语对象/映射变量（`const zh/en = {…}`，ContextLensPanel tierLabel 形）、语言条件三元/if/switch（207 处）、`…Zh/_ZH` 命名常量对。
- **迭代校准三轮**：v1 误报 `tr()` 通道（1068→235）→ v2 补实参对（235→170）→ v3 补 `??` 回退展开与数组实参。普查器非入库物，判据全部沉淀进守卫测试。

## 缺陷定性（最终 170 候选）

- **真缺陷 40 处**：EN/ja 用户可见裸中文，13 文件。
- **豁免面 130 处**（逐站人工定性，全部落台账）：App 启动错误屏**刻意中英并排**（App.tsx:478 注释备案「语言未知故双语并排」——核上下文避免破坏设计意图的误修）；commands.ts 关键词搜索别名（有意双语检索词）；truth-display 中文格式解析与 zh 资格注册表（EN 镜像在场）；style-fingerprint 内容语言原生特征名（EN_RHETORICAL_PATTERNS 对偶 + Rust 镜像 + golden 锁定）；LLM prompt/落盘 markdown（引擎面）；console 开发者日志；write-stopped 双端契约字面（642 号，须双栈同步轮）；ErrorBoundary 刻意零依赖（文件头备案）；PendingHooksView CSS 键映射；LanguageSelector 自指语言卡；error-copy zh 映射目标（本文件职能即 EN 协议→zh 展示）；产品专名等。

## 修复清单（40 处 / 15 文件）

1. **server.ts 409 协议面**：`CHAT_REQUEST_ALREADY_RUNNING` 消息 zh→EN（对齐错误协议全 EN 族惯例 2509/5449/6733），zh 展示由 error-copy 本地化映射承接（use-api/runtime 统一入口，行为对 zh 用户不变）；全仓唯一锚点核查后改文。
2. **词典通道（t）**：use-i18n.ts 新增 16 键双语——BookSidebar 运行态空态/文件不存在/章节标号、StoryPlayer 五面（开始游玩/节点缺失/已解锁结局/重新开始/此路不通）、Dashboard 未配模型四件（新手向导复用 nav.onboarding）、ChatPage 模型选择三件、BookDetail best-of-N 提示、ProjectSettings secret 占位。
3. **内联双语通道（tr）**：ExportBar 导出四件、PlayHud 关系/因为/世界时间/经过/同步（模块级 buildView 纯函数）、service-detail-state 校验五消息、DirectorPanel 选用通知、RecallTestCard 语义/词法徽标、WritingStatsCard 柱状 title、quick-open 章节标号；BookSidebar 运行态空态在组件外模块函数走 tr。

## 守卫落地（新增 `src/__tests__/i18n-hardcoded-guard.test.ts`，自足单文件对齐 727 号形态）

- **三判据**：①分类器夹具自检——10 惯用形+必红语义例逐一定性（裸中文/JSX 裸文本两例=恒绿防潜，848 号形状卫兵同族）；②台账外零容忍+台账内不超预算（新增即红，红报含可行动修复指引）；③台账预算=实况同步（清偿后预算须下调，防台账虚高掩盖新增）。
- **台账 17 文件**：每文件预算+定性理由（理由必须含证据链，禁空由）。
- **NEUTER1004 必红活体**：StoryPlayer 死局文案临时退回硬编码→台账外零容忍面恰红 1 项（含指引）→互逆恢复 3/3 复绿——分形语义自洽（夹具例不受影响）。

## 门禁终态

- typecheck 双 tsconfig 过（守卫测试 ts.Expression 品牌类型勘误先于全套件）
- studio 全套件 **871 例全绿**（868→+3）
- 727 号键完整性守卫绿（16 新键全注册）
- U+FFFD 扫描净
- Rust 树零改动（引用 1003 定桩）

## 备案

- server.ts pick 数组 6 处（329-334）为合规形的分类器盲区（`.join()` 链未展开），预算内台账化，后续分类器增强清偿时预算同步下调。
- server.ts 材料标题「（库资产）」后缀 1 处：展示面但需 lang 管道铺设，台账化待引擎面 i18n 专项。
- 条件挂载族/0GC/契约面未触碰；本轮为纯展示层 i18n 收敛，双栈契约面（write-stopped）明确留给双栈同步轮。

## 教训

1. **修 i18n 缺陷先核设计意图**——「启动屏双语并排」是备案过的刻意形态，无上下文核对会误修（豁免定性=上下文证据，非字面形状）。
2. **普查器判据=通道枚举**——仓库存在 t/tr/pick/nav/tuple/sibling-en/map-var/ternary 八通道，分类器漏一条就产生一整族误报；逐轮校准（误报面收敛 1068→130）比一次写全更可靠，最终判据必须沉淀进夹具自检。
3. **台账守卫三判据缺一不可**——只查「不超预算」会漏文件新增（台账外零容忍补）；只查台账会漏预算虚高（实况同步补）；分类器自身须夹具防恒绿。
4. **vitest 不做类型检查**——守卫测试绿≠typecheck 绿，ts 品牌类型（Node vs Expression）须 typecheck 门禁捕获。
