# 730 号：a11y 第三面——settings 控件程序关联收口（729 另案转正）+ 上游巡检复核

> 目标线：自主持续开发。729 另案转正：settings 页实锤级控件程序关联；
> 上游巡检附带复核。

## 一、上游巡检附带复核

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——第五巡维持收口态。

## 二、settings 控件程序关联（实锤级清偿）

729 扫描 19 控件的分账处置收口：

- **实锤级（无 placeholder 的 5 控件）**：
  - 每章字数/目标章数/状态三控件——已有可见 `<label>`，补
    `htmlFor` + `id` 程序关联（语义关联 + 点击 label 聚焦控件双收益）；
  - 导出格式 select → `aria-label={t("common.exportFormat")}`（无可见
    label，词典既有键复用）；
  - 章节 Revise select → 已有 `title="Revise with AI"` 兜底（728 口径已
    计），不动。
- **次级（有 placeholder + 相邻可见文本的 14 控件）**：维持 729 判定
  （双重线索兜底；label 显式关联随面板重构另案）。

## 三、验收

- studio 689/689 全绿；typecheck 双 tsconfig 过；gate:ts 八步全绿。
- a11y 三面至此全覆盖：按钮 aria（728）→ lang/file input（729）→
  settings 控件程序关联（本批）。

## 涉及文件

- packages/studio/src/pages/BookDetail.tsx（三 label htmlFor+id、导出
  格式 aria-label）
