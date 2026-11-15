# 1425 号 · gate:ts 十腿补全 + semantic-patterns 稳态终审 + bench 负载卫兵活体轮

> 1424 出口执行。1424 已收口前四腿（build/typecheck/test/audit:npm）+ Rust 全目标 + sweep + clippy + 四站活体走查；本轮补齐其余验证面，零代码改动。

## 一、gate:ts 完整十腿复验（全绿，含 1424 修复树）

build 19.8s / typecheck 19.3s / test 106.5s / audit:npm 5.2s / rust-bin 0.3s（活体二进制与 engine-rs 源码同源、cargo 指纹核验新鲜）/ node-fallback-smoke 35.3s / **engine-contract-diff 36.0s（双栈差分器零分歧）** / export-epub-smoke 35.0s / work-alternation 5.1s / check-routes 0.1s —— `[gate-ts] ✓ 全部门禁通过`。1424 唯一代码改动面（src-tauri ignored 测试 + sweep 预算）在十腿全谱下零回归。

## 二、audit:semantic-patterns 稳态终审（exit 1 = 工具本义，非红）

- 现树 findings=41（=850 清零备案稳态 42→41）；`forbidden remains`=0（禁入路径/禁改 token 零命中）。
- **字节级漂移证明**：850 时点（8aa1ae60）worktree 跑当时版本脚本 → 候选集与当前去行号逐条 diff 为空——**850 以来零漂移**；其间 6 个候选文件有改动（agent-session/agent-tools/film-authoring/tool-arguments/server.ts/BookDetail）但候选集逐条不变。
- **新代码零候选**：1169 四站文件（ChatPage/Sidebar/RecallTestCard/AntiAiAndExperience）grep 计数 0；1424 零 TS 改动。
- 处置=850 已裁决的语义启发式人审候选稳态（exit 1 为 `findings.length>0` 的工具本义，要求人审非门禁红）；不扩面、不摘条目。

## 三、bench:gate 负载卫兵活体（拒跑=守卫正确工作）

负载前置检查拒跑：loadavg 4.68/18 核=26%/核 > 20% 阈值。负载源=用户桌面活动（WindowServer 44%+ZCode UI），**非本轮残留**（1424 清场 pgrep 已证零残留）。按纪律不 `--ignore-load` 强跑（满载采样=噪声，严禁误判/洗基线）；热路径零改动面（1169=UI 事件守卫、1424=测试侧，均不在 criterion 基准路径）——待静默时段例行重跑，卫兵活体实证非恒绿。

## 四、其余独立卫兵（全绿）

- verify:publish-manifests：core/cli/studio 三包 OK。
- verify:engine-bindings：205 测试 passed + 186 bindings 导出全绿（61.9s）。
- 工作区洁净 + HEAD=1c0e8b38 + 无并行在途物。

## 五、下一号自 1426 起

出口=下一巡检周期（bench:gate 静默时段例行补跑到点）/ earendil-mz 单包合并（触发条件制已更新）；用户侧待办=Fork 推送（含 955-1425）。
