# 246 号：book-create staging 组合 e2e——231 提示词注入 + propose_action 确认卡联动验证

- 日期：2026-09-09
- 分支：develop
- 关联：230/231 号（提示词移植——本批为其真实链组合验证）、84 号（propose_action 确认卡）
- 推送核验：origin/develop 仍停 6bc65564——**201–245 共 45 提交待推送**；本批次后本地领先 46。两项默认值无新答复。

## 一、新增测试

`book_create_staging_prompt_and_propose_card`（mock LLM 全链）：
1. 创建 book-create 会话（无书）；
2. POST /agent 建书指令 → mock 捕获 system（断言含「建书助手」= 231 号 staging 提示词真实注入聊天循环）+ 按指令返回 propose_action create_book 工具调用；
3. 断言确认卡 details：kind=proposed_action / action=create_book / targetSessionKind=book-create / **sameSession=true**（TS sessionKind != chat 同语义——84 号 chat 面 false 的互补验证）。

## 二、验证

- engine：集成 **197**（+1）、lib 1315、clippy 零告警；其余门禁维持已绿。
