# 585 号：入库 README 死链修复——被引用的 8 份 docs 成品文档提升入库（.gitignore 白名单）+568 备案兑现

- 日期：2026-10-02
- 类型：docs+chore（入库链接完整性）
- 路线：580 号 README 对齐的直接后果清偿
- 状态：✅ 门禁全绿收口

## 缺口（入库缺陷）

README 三语的文档链接整排指向 `docs/*.md`——而 `docs/*` 整目录 gitignored（168 号本地文档策略）：**新 clone 用户拿到的入库 README 全部文档链接为死链**。8 份被引用文档（README 三语+docs 互引并集）= plugin-system / USER_GUIDE / QUICK_START / TROUBLESHOOTING / i18n-a11y / security-audit / signing-procurement / sea-feasibility。

## 裁决与交付

两政策冲突（本地文档策略 vs 入库门面链接完整性）裁决为**链接完整性优先**：

1. `.gitignore` 白名单例外：`docs/*` 之上 `!docs/<8 份>.md`——白名单精准性 dry-run 验证（仅 8 份 md 入库，superpowers/ 等其余仍忽略）。
2. 四份旧文档逐一头尾审阅定性为**成品**（i18n-a11y 策略 44 行 / security-audit 报告 78 行 / signing-procurement 指引 80 行 / sea-feasibility 决策记录 85 行），非工作稿，随白名单一并入库。
3. **568 备案兑现**：plugin_doc_contract.rs 去 skip 分支（文档已入库恒在场，护栏全环境恒生效）+模块 doc 更新。
4. 文档互引核验：plugin-system.md ↔ USER_GUIDE.md（578 新增插件节引用）入库后互指闭环。

## 门禁

plugin_doc_contract 1/1（无 skip 版）；clippy 双 0；cargo:testgate engine 45 目标 1895 + src-tauri 24 目标 583。TS 零改动。

## 下一号自 586 起
