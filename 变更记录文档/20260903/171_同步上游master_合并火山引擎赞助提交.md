# 171 · 同步上游 master：合并火山引擎赞助提交

- 日期：2026-09-03
- 模块：根 README 三语文件 + `assets/`（文档同步）
- 类型：chore（fork 同步上游）
- 关联：上游 [Narcooo/inkos] 09104838 `docs: add Volcengine sponsorship`；本仓 168 号（根 README 全量重写）

## 一、背景

GitHub Fork 页 Sync fork 提示：本 fork 的 `master` 领先上游 489 个提交、落后 1 个提交。落后的即上游 09104838——为三语 README 新增火山引擎赞助横幅与致谢段落，并入库图片 `assets/volcengine-agent-coding-plan.png`（纯新增 18 行，无删改）。网页端 Sync fork 对分叉历史只提供「Discard commits」（会丢弃本 fork 全部 489 个提交），故改走本地 merge 路线：匿名 fetch 上游 → merge 进 master → Fork 图形端推送。

## 二、改动

- `git remote add upstream https://github.com/Narcooo/inkos.git` 后 `git fetch upstream master`（匿名 fetch 可用，首次 SSL 抖动重试即成功）。
- `master` merge `upstream/master`（合并提交 4f1f530f），冲突与解决：
  - `README.md` 冲突（唯一冲突文件）：168 号全量重写后，上游侧整块旧正文（Kimi 公告、火花数据提示、v1.8.0 章节等）与重写内容对不上。解决：保留本分支版本，丢弃上游旧正文；在文末「致谢与许可证」节新增一条「赞助上游」致谢（含火山方舟链接），与 en/ja 保持三语一致。
  - `README.en.md` / `README.ja.md`：未重写过，自动合并成功（各自 +6 行赞助横幅）。
  - `assets/volcengine-agent-coding-plan.png`：新文件，自动并入（en/ja 横幅引用）。
- `upstream` remote 保留，后续同步可直接 `git fetch upstream && git merge upstream/master`。

## 三、验证

- `git log --graph`：本分支 489 个提交（至 170 号）完整保留，09104838 经合并提交 4f1f530f 并入，无 rebase / 无历史改写。
- `README.md` 无冲突标记残留，行文为桌面分支叙事 + 新增一条致谢；en/ja README 与上游该文件一致。

## 四、遗留与说明

- 推送需在 Fork 图形端执行（本机 CLI 无 GitHub 凭据，见既有约定）；推送后网页 Sync fork 应显示仅领先、不落后。
- 同日新建的 `develop` 分支仍停在与 170 号（889b964f）对齐的快照点，未包含本次合并；如需跟进可在 develop 上 `git merge master`（快进）。
