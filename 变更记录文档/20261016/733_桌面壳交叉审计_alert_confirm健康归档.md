# 733 号：桌面壳×前端交叉审计（alert/confirm 壳可用性健康归档）+ 上游第七巡

> 目标线：自主持续开发。候选「桌面壳深化新证据项」执行轮：前端原生
> 对话框依赖在 Tauri 壳的可用性审计（壳×前端交叉面）。

## 一、审计结论（健康态，配置面已前置预防）

- **使用面**：studio 前端 alert( × 25、confirm( × 6（BookDetail 导出
  成功提示/删除确认等）。
- **壳配置实况**：`tauri-plugin-dialog = "2"`（Cargo.toml）+
  capabilities/main.json **`dialog:default` 已授权**——dialog 插件覆写
  `window.alert/window.confirm` 为 plugin:dialog|message|confirm 调用，
  前端原生对话框调用在桌面壳形态**正常走原生对话框**。
- capabilities 描述并前置预防了同类 ACL 陷阱（notification 的
  unhandledrejection 误报先例 + 「dialog:default 同理预防」注记——146 号
  「内建脚本调用也过 ACL」教训的配置面固化）。
- **结论**：壳×前端交叉面**无实锤缺陷**——25+6 处调用在 Web dev 形态
  （浏览器原生对话框）与桌面壳形态（plugin 原生对话框）双形态可用。
- UX 风格面（原生对话框 vs 自研弹层视觉一致性）非缺陷不扩。

## 二、上游第七巡

fetch upstream 双 master 判据复跑：落后 upstream/master = 0，全部分支零
落后——第七巡维持。

## 三、教训

交叉面审计（前端调用 × 壳能力配置）要先查**配置面是否已预防**——
capabilities 注释里的前置预防记录（「dialog:default 同理预防」）说明该面
在 146 号时代已被处置；重复审计的价值=确认预防仍然在位。

## 涉及文件

- 本件（审计归档，零代码改动）
