# 721 号：上游 2.0 分支合并巡检——interactive-fiction 分支树工作流挖出（下批合并审阅）

> 目标线：自主持续开发。候选方向「上游 2.0 新分支合并巡检」执行轮；
> 前次全量清账（41ca307e，2026-10-16）后的增量巡检。

> **⚠️ 722 号勘误**：本报告「feature/interactive-fiction 15 独有提交挖出」
> 系判读错误——15 提交是相对**上游 master** 的独有，而该分支**早已并入
> 我们 master**（41ca307e 全量清账时代，merge-base --is-ancestor 实证
> IS-ANCESTOR；影游域 interactive-film sessionKind 即其来源）。正确巡检
> 结论=全部上游分支对我们零落后，722 号勘误批次已归档。方法论教训：
> 分支独有性必须对比**双 master**（上游 master 与我方 master），只比上游
> master 会把「我方已并而上游未并」误判为待合并。

## 一、巡检事实（git 实证）

1. **upstream/master 完全对齐**：merge-base = 8fc2ae57 = upstream/master HEAD
   本身——上游 master（含 PR #400 creative-harness-v2 合并）已是我们的祖先，
   **零落后**。
2. **两分支已并入 master 零独有**：codex/fix-studio-cancel、feat/llm-providers
   → `upstream/master..分支` 均为 0 提交，无需动作。
3. **feature/interactive-fiction：15 个独有提交，+3334/-419、30 文件**——
   互动影游**分支树工作流 v1**（此前 41ca307e 收口记录的 mining 候选实锤）。

## 二、interactive-fiction 分支内容速览（下批合并审阅输入）

- **模型面**：interactive-fiction.ts（54 行新模型）+ book.ts 分支字段（+8）。
- **代理面**：choice-generator.ts（178 行新代理——选项生成；呼应 41ca307e
  记录的 choice-generator/choice-auditor 设计候选）。
- **管线面**：runner.ts +309/agent.ts +89——分支书创建/续写分支选择/
  分支树持久化（state/manager.ts +254）。
- **CLI/工具面**：interactive branch CLI commands + agents 分支工具暴露。
- **守卫面**：block rewrite for interactive branch books（互动书禁重写——
  551c8ab9）/ branch choice 前置校验（12ddf287）。
- **测试面**：interactive fiction v1 E2E flow（77eec320）+ 文档两件。
- **基建同向**：44b5e52e untrack docs/+gitignore 与我们 docs 策略同款。

## 三、合并预备裁决（三档法预评估，正式裁决归 722 批验收链）

- **倾向档**：真特性合并（非 `-s ours`）——互动影游分支树是 2.0 先进特性域
  （fork 产品引擎 interaction 面仅 HTTP 惰性会话，无分支树工作流；TS 侧
  ChatPage/影游 UI 有消费土壤）。
- **冲突预警**：runner.ts/manager.ts/book.ts/index.ts 与 709/714/717 批的
  我们侧改动**强冲突区**（status/auditIssues/writing 键、writeDraft/
  generateDirections 等新增方法同文件）——合并须逐 hunk 复核双侧新增共存。
- **验收链预备**：合并后必跑 gate:ts 八步 + contract-diff（68 面不回退）+
  bindings + E2E flow test（上游自带 77eec320）+ 双端 duel。
- **联动面**：choice-generator 若采，Rust 引擎侧对偶（interactive 分支树
  的 Rust 面）评估另立工项（产品引擎双栈一致性）。

## 四、收尾事项（用户侧）

- **Fork 图形端推送**：本地累计 **14 个提交**（febe8fdd 起 712-720 各批次
  至 e4bace58）待推送——本机无 GitHub CLI 凭据，请打开 Fork 对 origin/master
  推送并核验。
