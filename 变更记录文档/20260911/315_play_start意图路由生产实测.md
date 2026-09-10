# 315 号：play_start 意图路由生产实测——意图验证矩阵扩展至互动世界

- 日期：2026-09-11
- 分支：develop
- 关联：303/304 号（意图验证面——本批延续至 PlayStart）、225 号（play 域原子写——落盘面实证）
- 编号衔接：查 20260911 目录最大号 314，顺延 315
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–315 共 15 提交待推

## 一、生产引擎实测（POST /agent + requestedIntent=play_start，button 来源）

载荷：雪夜古宅（premise/worldContract/visualContract/mode=open/initialScene/suggestedActions×3）。

| 层 | 结果 |
|---|---|
| 意图路由 | ✓ `play_start` tool **status: completed** |
| 世界落盘 | ✓ `worlds/{session}/world.json`（title=雪夜古宅、mode=open）+ `runs/main/`（play-graph.json、state/current.json、transcript.jsonl、projections/scene.md+state.md） |
| 内容正确性 | ✓ 投影 scene.md = 初始场景逐字；current.json 契约/模式字段齐全 |

## 二、意图验证矩阵现状

| 意图 | 白名单 | 分派 | 生产实测 |
|---|---|---|---|
| create_book | ✓ | ✓ | ✓（291/312 聊天全链） |
| write_next | ✓ | ✓ | ✓（292 UI 全链） |
| short_run / script_create / storyboard_create / interactive_film_create / generate_cover / translation_create | ✓ | ✓ | ✓（e2e 既有覆盖 + 267 冒烟） |
| fanfic_init / continuation_import / spinoff_create / style_imitation | ✓（303） | ✓ | ✓（303/304） |
| **play_start** | ✓ | ✓ | ✓（**本批**） |
| play_step | 聊天工具面（非 confirm-production，设计如此） | — | 走 use play UI/工具链 |

## 三、验证性质与遗留

零代码改动批（纯实测+归档）。遗留：301–315 共 15 提交待推送；并行会话三件第四十二轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
