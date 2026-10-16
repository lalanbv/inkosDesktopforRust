# 731 号：a11y 第四面——次级控件 label 关联收口（settings 可访问名归零实证）

> 目标线：自主持续开发。729/730 另案的次级控件深化转正执行：面板族
> placeholder 控件 aria-label 全量补齐 + 活体复扫归零实证。

## 一、上游巡检附带复核（第六巡）

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——收口态第六巡维持。

## 二、次级控件 aria-label 全量补齐（18 处 + 2 处活体补遗）

- **面板族 17 处**（7 组件）：RecallTestCard 1 / AntiAiAndExperience 4 /
  SeriesCanon 4 / Deconstruct 2（含 121 行多行 tr 形态手工补）/
  Director 1 / AssetLibrary 4 / Codex 2——aria-label 同参镜像各自
  placeholder 的 tr(中文, 英文) 双语调用。
- **活体复扫补遗 2 处**：反AI「严重级别」select + 导演「运行模式」select
  （无 placeholder 的 select——729 实锤级清单漏计，DOM 上下文定位面板后
  tr 双语补齐）。
- **脚本化教训**：批处理正则两轮翻车实录（f-string 双重大括号产出
  `tr(tr(...))` 嵌套 + 中文含 ASCII 括号致行尾 `))}` 正则漏网）——
  **机械批改后必须 typecheck 全量验证**，行级精确修收尾（3 轮收敛）。

## 三、活体复扫归零实证（settings 页）

| 指标 | 修复前（729） | 修复后 |
| --- | --- | --- |
| input/textarea/select 总数 | 19 | 23（面板数据演进） |
| 无可访问名控件 | 19 | **0**（ariaTotal 22 个 aria-label 在位） |

残 2 select 活体补遗后**归零**。三主页面 a11y 全口径（button/img/input/
lang）可访问名面收口。

## 四、验收

- studio 689/689 全绿；typecheck 双 tsconfig 过；gate:ts 八步全绿。

## 涉及文件

- packages/studio/src/components/{RecallTestCard,AntiAiAndExperiencePanel,
  SeriesCanonPanel,DeconstructPanel,DirectorPanel,AssetLibraryPanel,
  CodexPanel}.tsx（aria-label 17+2 处）
