# 327 号：notify webhook 全链生产实测——守护进程写周期 → pipeline-complete 双载荷送达

- 日期：2026-09-11
- 分支：develop
- 关联：226 号（notify 超时修复——本批为送达行为的生产实证）、111 号（TS handleAuditFailure 暂停分支对齐）、315 号（daemon 面）
- 编号衔接：查 20260911 目录最大号 326，顺延 327。（注：322 号当日因误跳编号被并入 321 号重命名，该号空缺；台账以 git 历史为准。）
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–327 共 27 提交待推

## 一、生产引擎实测（daemon 写周期 + webhook receiver，全部通过）

环境：inkos.json `notify: [{type:"webhook", url:"http://127.0.0.1:1400/hook"}]` + 本机 webhook receiver + 书籍 fixture + daemon 生产链 mock。

| 步骤 | 结果 |
|---|---|
| `POST /daemon/start` → 写周期 | ✓ 守护进程循环启动并处理书籍 |
| 章节产出 | ✓ `chapters/0001_风起.md`（周期 1）、`0002_风起：林动.md`（周期 2，重跑） |
| **webhook 送达** | ✓ receiver 完整捕获 **2 条 `pipeline-complete`**：①守护进程级（bookId=""，markdown 格式，「✅ 通知书 第2章」）②书籍级（bookId="b1"、chapterNumber=2、passed/revised/status/wordCount 结构化 data） |
| 引擎韧性 | ✓ receiver 曾在死亡期被投递（dispatch 错误被 eprintln 容器化）——引擎/守护进程零崩溃（226 号容错设计的活体实证） |

## 二、结论

notify webhook 域生产链闭环实证：inkos.json 通道配置 → 守护进程写周期 → 事件分派 → 第三方端点送达。三条事件名（pipeline-complete/pipeline-error/diagnostic-alert）中 complete 已活体验证；error/alert 路径同分派器（代码同路）。

## 三、验证性质与遗留

零代码改动批（纯实测+归档）。receiver 的首版崩溃为本批临时脚本缺陷（ESM require 误用），与产品无关。遗留：待推送提交以 git 为准；并行会话三件在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
