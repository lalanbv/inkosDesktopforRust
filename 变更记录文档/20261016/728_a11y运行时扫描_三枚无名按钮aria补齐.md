# 728 号：a11y 运行时扫描——三枚无名图标按钮 aria 补齐（TS 侧续查第三面）

> 目标线：自主持续开发。TS 续查第三面：可访问性（a11y）——静态口径假阳性
> 高，改用**运行时 DOM 口径**（走查环境 + 浏览器逐页扫描）。

## 一、扫描方法与口径演进

- 静态口径（`<button` 开标签缺 aria-label/title）→ **297 候选假阳性极高**
  （绝大多数按钮有可见文本子串，静态无法判断）——弃用。
- **运行时口径**：walkthrough-env + 浏览器逐页扫描
  `button` 无 innerText 且无 aria-label 且无 title 的实例——三主页面
  186 钮实锤仅 **3 枚**（首页 1/聊天页 2/设置页 0）。

## 二、修复（三枚 aria 补齐，按既有 i18n 惯用）

1. Dashboard 书卡「更多」菜单钮（MoreVertical）→ `aria-label={t("book.moreActions")}`
   + 词典新增 `book.moreActions`（更多操作/More actions）。
2. ChatPage 发送钮（bg-primary 圆钮，空输入 disabled 态也无名）→
   `aria-label={isZh ? "发送" : "Send"}`（对齐同文件「添加 Skill」双语字面惯用）。
3. BookSidebarToggle ContextDock 移动端开合钮 → `aria-label={t("nav.openContextPanel")}`
   + 词典新增（该组件独立无 isZh，走 t prop + 词典键路径）。

## 三、活体复验（修复前后对照）

| 页面 | 修复前 unnamed | 修复后 unnamed | aria 实证 |
| --- | --- | --- | --- |
| 首页 | 1 | **0** | 「更多操作」在位 |
| 聊天页 | 2 | **0** | 「发送」「打开上下文面板」在位 |
| 设置页 | 0 | 0 | — |

## 四、验收

- studio **689/689** 全绿（i18n-key-integrity 1 测同步守新词典键）；typecheck
  过；gate:ts 八步全绿。

## 五、教训

1. **a11y 扫描口径决定信噪比**：静态形态扫（297 假阳性）vs 运行时 DOM 扫
   （3 实锤）——有走查环境时直接上运行时口径。
2. tab 反复开关后 IAB 会话状态漂移（hash 空/total 0）——关全部标签重开
   即净（718 教训「异常风暴先换标签」的极端形态）。

## 涉及文件

- packages/studio/src/pages/Dashboard.tsx、ChatPage.tsx、
  components/chat/BookSidebar.tsx（三枚 aria）
- packages/studio/src/hooks/use-i18n.ts（两个新词典键）
