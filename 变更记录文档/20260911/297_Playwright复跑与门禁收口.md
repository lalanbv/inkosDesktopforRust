# 297 号：290 号后 Playwright 复跑——全仓门禁整合状态收口

- 日期：2026-09-11
- 分支：develop
- 关联：290 号（exportHref 删除——本批验证其 e2e 零回归）、275 号（上轮整合复跑先例）
- 编号衔接：查 20260911 目录最大号 296，顺延 297
- 推送核验：origin/develop = a1b34a33 未动；本地领先 274–297 共 24 提交待推

## 一、复跑结果

**Playwright 36/36 通过（1.8m）**——290 号 BookDetail.tsx 死代码删除后的 e2e 回归保险，当前 HEAD 全部门禁重新对齐：

| 门禁 | 状态 | 最近验证 |
|---|---|---|
| engine-rs cargo test（INKOS_DUEL=1） | ✓ 1625 | 286 号批内 |
| engine-rs clippy --all-targets | ✓ 0 | 286 号批内 |
| TS 三包单测（core/studio/cli） | ✓ 202/90/42 文件 | 278 号 + 290 号 studio 复验 |
| TS typecheck | ✓ 0 错 | 290 号批内 |
| Playwright e2e | ✓ 36/36 | **本批** |
| 脚本门禁（manifests/bindings/semantic） | ✓ | 275 号批内 |
| bench 基线 | ✓ 272 号安静窗复跑有效 | — |
| 依赖审计 | ✓ Rust 0 漏洞（263）+ Node 23 残留定性（278） | — |

## 二、验证性质与遗留

零代码改动批。遗留：274–297 共 24 提交待推送；并行会话三件第二十六轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
