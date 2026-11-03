# 879 号：工程轮——dark 模式色卡修复 + a11y spec dark 主题抽查落地

日期：2026-11-04。性质：**实质工程轮**（878 号夜间全量暴露的三处 dark 色卡缺陷修复 + dark 主题抽查用例落地）。

## 一、dark 色卡修复（878 暴露清单）

| 页 | 878 暴露值 | 元素 | 修复 |
|---|---|---|---|
| chapter | destructive 深底 3.46 | ChapterWorkspacePanel「删除本章」危险动作钮（text-destructive） | `dark:text-red-400`（深底 ≈7.7） |
| service-detail | 3.76 | ServiceDetailPage「删除配置」危险动作钮（同形） | 同上 |
| doctor | amber 深底 2.54 | 878 已补 `dark:text-amber-400`（夜间跑在其之前）——本轮 dark 抽查证实已达标 | 无需再改 |

**范围裁定**：`text-destructive` 全站多处用途，dark 提亮走**元素级 dark: 分支**而非全局 `--destructive` 变量提亮——后者会连带 `bg-destructive`+白字的按钮对（按钮底变亮→白字对比下降），风险面不成比例。其余页面的 dark destructive 用途由 dark 抽查扩展时逐页数据驱动（878 同纪律：只修实测命中）。

## 二、a11y spec dark 主题抽查落地（三页）

chapter/service-detail/doctor 三页：addInitScript `inkos:studio:theme="dark"`（主题实现=dark 类挂 html，不依赖 prefers-color-scheme，headless 确定复现）+ V2 壳。两阶段断言：①必红探针检出（dark 下检测器活性）；②移除探针后干净审计零违规（region 备案项除外）。

**首轮探针两坑（修复留档）**：
1. **axe color-contrast 跳过视口外元素**——探针 append 到 main 尾部在页面底外，只剩 button-name 检出（color-contrast 缺席）。修=容器 `position:fixed;top:0;left:0` 定位回视口（白底子元素保持违规样本语义）。
2. **探针违规样本混进零违规断言**——探针自带的无名钮/低对比文本/白底容器 region 违规全被计入，dark 抽查永远红。修=两阶段分离（探针断言 arrayContaining → remove → 干净审计断言全零）；白底容器从 body 根移入 main 内（region 由地标覆盖）。
3. axe 注入时序：探针 evaluate 前须 `addScriptTag`（auditViolations 内部的注入在探针 evaluate 之后才发生）。

## 三、验证

| 面 | 结果 |
|---|---|
| dark 抽查（3 页×两阶段） | ✓ 3/3：必红检出（button-name+color-contrast）→ 干净审计零违规 |
| a11y spec 全量 | ✓ **61/61**（浅色 58 基线不受主题固定影响 + dark 3） |
| studio vitest | ✓ 755 |
| `gate:ts` 八步 | ✓ 全绿 |

## 四、备案

- dark 抽查现仅三页——扩展至全路由×dark 为后续周期工项（本轮三页已把 878 暴露的 dark 缺陷闭环；其余页面 dark 状态未审计=未知而非已证合格）
- doctor 页浅色 amber-800（877 修）经本轮 dark 抽查页同源验证：dark 下走 dark:text-amber-400 分支 ✓
- 浅色必红自检探针（body 注入+arrayContaining 宽松断言）维持原样——宽松断言下 region 混入无害

**下一号自 880 起；出口不变；用户侧待办=Fork 推送 12 个提交（至本提交）。**
