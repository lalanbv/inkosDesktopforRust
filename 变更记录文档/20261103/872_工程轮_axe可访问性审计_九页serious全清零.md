# 872 号：工程轮——axe 可访问性审计：九页 serious 级违规全清零

日期：2026-11-03。性质：**实质工程轮**（UI 备案三项之二「axe 可访问性引入」，150 号规划确认过的真实需求；工作区预设已于 869 交付）。

## 一、审计方法（真浏览器注入，非 jsdom）

axe-core 4.10.3（cdnjs 正品，隔离目录 /tmp/axe-core-audit/）经带 CORS 头的本地 HTTP 服务注入走查页（fetch→eval），walkthrough-env Rust 引擎三件套拉起，真 Chromium 渲染（jsdom 测不了对比度/焦点顺序）。九个主要页面：dashboard/book/timeline/radar/services/settings/logs/genres/doctor。

**首轮违规定性：四类系统性问题（全页同形=修复一处全局消失）**：

| 违规 | 级别 | 元素定位（elementFromPoint+祖先链） | 修复 |
|---|---|---|---|
| aria-prohibited-attr ×9 页 | serious | StatusBar `StatusDot`——无 role span 带 aria-label | `role="img"`（状态点=图形化指示语义成立） |
| color-contrast 3.15 | serious | TabStrip「预览」徽标 `text-muted-foreground/60` | `text-foreground/70` |
| color-contrast 2.58 ×2 | serious | ContextDock 章节列表字数（ChaptersSection:52）`text-muted-foreground/50` | `text-foreground/70` |
| color-contrast 2.01 ×4 | serious | QuickActions chips `text-muted-foreground` 叠 `bg-secondary/50` | `text-foreground/80`（批量复测时暴露——首轮被 dock 弹层遮挡未计入） |
| page-has-heading-one | moderate | book/chat 页无 h1 | ChatPage 主容器顶部 `sr-only h1`（读屏 H 键直达主区） |
| region ×3 | moderate | TabStrip 容器无 landmark（2 个为 Base UI dialog-header sr-only） | TabStrip `role="navigation"+aria-label` |
| heading-order | moderate | RadarView h1→h3 跳级 | 3× `<h3>` → `<h2>`（样式不变只改语义层级） |

修复面 6 文件：StatusBar.tsx / TabStrip.tsx / ChaptersSection.tsx / chat/QuickActions.tsx / pages/ChatPage.tsx / pages/RadarView.tsx。

## 二、复测终态（九页）

**serious 级违规全清零**（aria-prohibited-attr ×9→0、color-contrast 全页 0）；heading-order 清零；唯一残留 = **region(moderate)×2 全页恒定** = Base UI Dialog 的 sr-only dialog-header 结构飘在 landmark 外——**备案**：框架内部结构，侵入成本 >> moderate 收益，随 Base UI 升级观察。

**时序假信号教训**：book 页 color-contrast×4 在九页批量循环（1300ms/页）出现、足时 2500ms 单独复测消失——ChatPage 异步取数中间态元素被 axe 命中。与 bench-gate-load-noise（满载假红）/866（负载假红）同族：**时序敏感项判确定性必须足时隔离复跑**。axe 批量审计每页等待须足时或等网络静默。

## 三、验证

| 面 | 结果 |
|---|---|
| studio vitest | ✓ **749 passed（140 文件）零回归** |
| `pnpm typecheck` | ✓ 双 tsconfig |
| `gate:ts` 八步 | ✓ 全绿（build/typecheck/test 111.7s/audit:npm/rust-bin 活体/三冒烟） |
| axe 复测（真浏览器） | ✓ 九页 serious=0 / moderate=2（备案项恒定） |

## 四、范围备案

- **未审计面**：play/film-studio/flow/logs 深层页/onboarding 等未入本轮页面集；`text-muted-foreground/50` 同族在 PlayHud×4/BookSidebar/SessionBranchSwitcher 等未命中小节——下一审计周期扩展页面集时同法清扫（改动需逐处视觉走查，不宜盲改）
- axe 工具链为临时注入（不入 devDependencies）；固化防回归门禁待下一审计周期评估（页面集稳定后）
- Rust/bench 零触碰

**下一号自 873 起；出口不变；用户侧待办=Fork 推送 4 个提交（0069adcc/a4b1f788/0d2e0c45/本提交）。**
