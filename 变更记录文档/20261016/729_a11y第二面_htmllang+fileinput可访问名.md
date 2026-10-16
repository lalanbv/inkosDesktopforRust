# 729 号：a11y 第二面——html lang + file input 可访问名（运行时扫描实锤修复）

> 目标线：自主持续开发。728 号按钮面之后的 a11y 第二面：img alt / input
> 可访问名 / html lang（运行时 DOM 口径同法，三主页面扫描）。

## 一、扫描实况（三主页面）

| 检查项 | 实况 | 判定 |
| --- | --- | --- |
| img alt | 三页 **0 个 `<img>`**（图标全 inline SVG） | 面不存在 |
| input 可访问名 | 首页 0；聊天页 2（双 file input）；设置页 19 控件 placeholder/相邻可见文本兜底 | **聊天页 file input 实锤**；settings 属次级（有可见邻近文本，程序关联缺失为 WCAG AA 技术项另案） |
| html lang | `<html lang="en">` 与中文内容主体不符（屏幕阅读器按 en 朗读中文=发音错误） | **实锤** |

## 二、修复（三处）

1. `index.html` lang="en" → **lang="zh"**（内容主语言）。
2. ChatPage 文件夹导入 input（folderInputRef，webkitdirectory）→
   `aria-label` 导入章节文件夹/Import chapter folder。
3. ChatPage 附件上传 input（fileInputRef）→ `aria-label` 上传附件文件/
   Upload attachment files。

## 三、验收

- studio 689/689 全绿；typecheck 双 tsconfig 过；gate:ts 八步全绿。
- settings 页 19 控件的 label 显式关联（WCAG AA 技术项）记录为次级另案
  ——有相邻可见文本兜底，非裸无名面。

## 涉及文件

- packages/studio/index.html（lang）
- packages/studio/src/pages/ChatPage.tsx（两处 file input aria-label）
