# 326 号：release profile 冒烟——发布档二进制（301–319 全部变更）+ 入库 mock 资产双验证通过

- 日期：2026-09-11
- 分支：develop
- 关联：273–319 号（release 未重编前的代码变更面）、313/314 号（walkthrough-mock 入库资产）
- 编号衔接：查 20260911 目录最大号 325，顺延 326
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–326 共 26 提交待推

## 一、背景

target/release 二进制自 263 号时代构建后，301–319 号的全部引擎变更（body 上限/arcs 形状/意图路由/续写语义/翻译收敛等）**从未经过 release profile 编译与运行**——debug 门禁绿不等于 release 编译零警告零错误。

## 二、结果（全通过）

| 步骤 | 结果 |
|---|---|
| `cargo build --release`（增量重编 35s） | ✓ 零错误（release profile 编译干净） |
| release bin 启动 + health | ✓ |
| **fanfic 意图全链**（用 313/314 号入库 walkthrough-mock 资产） | ✓ 「同人创作完成：《发布档验证》」+ fanfic_canon.md（正典）+ story/outline/story_frame.md（地基 5 段）落盘 |

双重意义：①release profile 的编译与运行验证补齐；②入库 walkthrough-mock 资产的**发布档**交叉验证（其同人架构师分派修复——314 号——在 release 下同样正确服务地基格式）。

## 三、验证性质与遗留

零代码改动批（构建+冒烟）。遗留：301–326 共 26 提交待推送；并行会话三件第四十七轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
