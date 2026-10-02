# 587 号：gate:ts 步骤序根因修复——build 前置（core dist 陈旧瞬态假红整类根治）

- 日期：2026-10-02
- 类型：chore(scripts)（门禁健壮性）
- 路线：572/581 两次实测坑的根因清偿
- 状态：✅ 门禁全绿收口

## 问题（两次实测坑同一根因）

studio/cli 的 typecheck 消费 core 的 **dist**（package main 指向发布产物），而原步骤序 typecheck 第 1 步、build 第 4 步——core 源码改动后若 dist 未重建，typecheck 产生「**源码已对但类型红**」的瞬态假红：

- 572 号：skills-endpoint 测试 TS2339，隔离复跑绿（当时误判为并行负载）；
- 581 号：core 改动后直接跑差分器消费陈旧 dist（当时归因「需 filter build 再验」并入库为教训——**两次都当环境问题绕过，无人动步骤序**）。

## 修复

`gate-ts.mjs` 步骤重排：**build 前置为第 1 步**（产物链先行 → typecheck/test/活体套件全部消费新鲜 dist），头注释同步。`--fast` 仍跳过 build（快速信号折衷，dist 陈旧风险在脚本内备案）。

## 验证

- 全量 gate:ts 七步全绿（汇总显示 build 列首：✓ build→✓ typecheck→…）；
- gate:ts:fast 四步全绿（build/typecheck/test/audit:npm，fast 折衷口径确认）。

## 教训

「隔离复跑绿」类的瞬态假红在归档环境解释之外，应**追问确定性根因**（本例：步骤序使 dist 时序必然偶发）——572/581 两次归档为负载/教训注记，直到第三次复盘才定位步骤序。

## 下一号自 588 起
