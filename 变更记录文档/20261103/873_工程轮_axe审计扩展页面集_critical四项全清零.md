# 873 号：工程轮——axe 审计扩展至未审计页面集 + /50 同族清扫：critical×4 全清零

日期：2026-11-03。性质：**实质工程轮**（872 号备案项清偿：扩展页面集 + muted-foreground/50 未命中同族），沿用 872 号真浏览器注入方法与足时复测纪律。

## 一、/50 同族清扫（5 处文本元素，统一 text-foreground/70=872 实测通过组合）

PlayHud.tsx emptyText（579）/detail.label×2（621/685）、BookSidebar.tsx「文件不存在」（168）、SessionBranchSwitcher.tsx 时间戳（186）。ChevronDown 图标（675）不修——svg 无文本 axe 不测，装饰性。

## 二、扩展页面集审计（onboarding/play/film/film-studio/flow/import/truth/chapter/analytics）

真缺陷四项全修：

| 违规 | 级别 | 元素 | 修复 |
|---|---|---|---|
| StoryGraphTree 场景 textarea 无可访问名称 ×3 | **critical** | `data-testid="film-scene-n1..n3"` | `aria-label={tr("场景描述","Scene description")}` |
| ImportManager 目标书籍 select 无名称 | **critical** | `<select>`（placeholder option 不算名称） | `aria-label={t("import.selectTarget")}`（既有 key 复用） |
| StoryGraphTree/FlowView 无一级标题 | moderate | 两页 | 各加 `sr-only h1` |
| React Flow attribution 水印对比度 2.8 | serious | `.react-flow__attribution a`（库内置 #999999） | CSS 覆盖 `color:#736a61!important`——**保留水印**（React Flow 许可条款要求 attribution），仅调对比度；首版被 react-flow 自身样式特异性压过（computed 仍 #999），提升为 `.react-flow .react-flow__attribution a + !important` 后生效（computed #736a61 实测） |

## 三、终态（新鲜 bundle，十八页全站复测）

**critical/serious 全站清零**。唯一残留 = `region(moderate)×2` 全页恒定（872 已备案：Base UI dialog-header sr-only 框架内部结构）。book 页批量循环中的 color-contrast×4 为已证实时序假信号（872 号足时复测消失）。

## 四、新教训：陈旧 bundle 污染审计（本轮最大方法论收获）

第一轮扩展审计时五页（truth/chapter/analytics/flow/import）集体渲染 ErrorBoundary 错误屏（componentStack `at Lazy`）——**不是代码缺陷**：dist 重建落盘后浏览器内存中仍是旧 bundle（hash 导航不重载页面），循环 goto 触发 lazy chunk 加载时引用已被新 build 删除的旧 chunk 文件名 → 404 → React lazy 抛错。判别过程：stash 873 改动 rebuild 复测「正常」→ pop 恢复后 reload/hash-nav 双路径复测「全部正常」→ 定性为审计方法学污染。**纪律补充：dist 重建后必须先 reload 页面（拉新 HTML 换 bundle）再注入 axe 跑审计**——与 872 足时纪律并列为本仓审计 SOP 双条。

## 五、验证

| 面 | 结果 |
|---|---|
| studio vitest | ✓ 749 passed（140 文件）零回归 |
| `pnpm typecheck` | ✓ 双 tsconfig |
| `gate:ts` 八步 | ✓ 全绿 |
| axe 终态（十八页） | ✓ critical/serious 全清零，残留仅备案 region×2 |

**下一号自 874 起；出口不变；用户侧待办=Fork 推送 5 个提交。**
