# 239 号：提示词 golden 差分守门——首次运行抓 6 处引号手抄漂移

- 日期：2026-09-09
- 分支：develop
- 关联：230/231/234 号（提示词移植——本批为其建立防漂移守门）、212/216/214 号（golden 模式第四次落地，再次立功）
- 推送核验：origin/develop 仍停 6bc65564——**201–238 共 38 提交待推送**；本批次后本地领先 39。两项默认值无新答复。

## 一、动机与实施

230/231/234 号的系统提示词为**手工转抄** TS 源码（500+ 行中英文），弯/直引号（TS 源码因字符串定界符两种混用）等不可见字符极易漂移。按 golden 模式建立事实源守门：

- core：`agent-system-prompt.ts` 导出 `buildGoldenPromptSnapshot(bookId)`（20 面：chat/book/edit×2/book-create staging/short/script/storyboard/film clarify/play 无世界 × zh/en）；
- 事实源：`golden/agent-prompts.json`（45KB，`REGEN=1` 再生）；
- core：`golden-agent-prompts.test.ts` 快照比对；
- engine：`tests/golden_agent_prompts_diff.rs` include_str! 同文件，调 `chat_prompts::build_*` 逐面断言。

## 二、首次运行即抓 6 处手抄漂移（并修复）

全部为**引号形态**：TS 各面源码因定界符不同，弯引号（\u201c\u201d）与直引号混用——chat.en 直、book.en 弯、play 无世界 en 直、short.en 直、book-create staging.en 直。逐处对齐 TS 真实输出（以 golden 为准，非猜测）。

## 三、验证

- engine：golden_agent_prompts_diff **1/1**（20 面逐字）、lib 1315、集成 196、clippy 零告警、duel 真跑 10/10。
- core：golden-agent-prompts 测试通过（1917 基础上 +1）。

## 四、遗留

无。提示词后续修订的流程：改 TS 源 → `REGEN=1` 重生成 golden → Rust 侧同步（差分测试拦截不同步）。
245 号补记：play 有世界/无世界两面均纳入 agent-prompts.json golden（22 面）——验证 80 号 play_chat_system_prompt 与 TS buildPlayPrompt(playWorldExists=true) 仍逐字一致（含铁律段/Output Rules），无需修改。
