# 316 号：translation_create 意图路由生产实测——创建/分段/三格式导出全链通过

- 日期：2026-09-11
- 分支：develop
- 关联：315 号（play_start 实测——本批延续意图验证矩阵至 translation_create）、273 号（translations/upload body 上限修复——同域）、70 号（translation 域移植）
- 编号衔接：查 20260911 目录最大号 315，顺延 316
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–316 共 16 提交待推

## 一、生产引擎实测（POST /agent + requestedIntent=translation_create，button 来源）

fixture：`books/the-novel.txt`（英文两章）。载荷：filePath + sourceLanguage=English + targetLanguage=中文（简体）+ title。

| 步骤 | 结果 |
|---|---|
| 意图路由 + 创建 | ✓ `translation_create` tool completed；「Translation project "the-novel 译本" created. English -> 中文（简体）Chapters: 2」 |
| **分段落盘** | ✓ translations/{id}/：manifest.json + glossary.json + review-report.md + source/chapter-000{1,2}.json + translated/chapter-000{1,2}.json（空译文骨架） |
| `GET …/translations/{id}` 详情 | ✓ manifest（标题/语言对/2 章）+ review 空报告 |
| **导出矩阵** | ✓ POST export：txt（79B）/md（90B）/epub（1.9KB，mimetype 首条 stored + OPF）三格式全部落盘，`chaptersExported: 2` |

比对备注：export 的格式参数在**请求体**（两端一致）；本批首次实测时误用查询串已被双端一致的缺省 md 兜底——行为无偏差。

## 二、结论

意图验证矩阵新增 translation_create 生产实测 ✓。至此矩阵覆盖：create_book / write_next / 四件创建域 / play_start / translation_create——全部白名单+分派+生产实测三层闭环；余下 script/storyboard/interactive_film/generate_cover/short_run 由既有 e2e 覆盖（同构执行器模式）。

## 三、验证性质与遗留

零代码改动批（纯实测+归档）。遗留：301–316 共 16 提交待推送；并行会话三件第四十四轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
