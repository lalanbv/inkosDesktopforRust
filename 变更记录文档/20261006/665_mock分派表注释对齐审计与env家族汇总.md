# 665 号：walkthrough-mock 分派表注释与实际分派链对齐审计（657 教训落地）+ pi1.0 key 复探

日期：2026-10-06。类型：docs(注释对齐审计，零行为改动)。前置：657 教训「分派表与真实 agent 清单的对齐审计应成为 mock 改动固定步骤」；650/655/660 多次改动分派链后的对齐审计。

## 一、审计发现与纠偏

逐分支比对头注释分派表与实际分派链（grep `sys.includes`/`lastRole` 全量）：**4 个分支漏记**（小说连续性分析师 650–652、状态追踪分析师、continuity validator、事实提取专家 483）；「其余 → PASS」描述过时（实际「已生成确认卡」）；「与 engine-rs 九路 agent 提示词对应」的说法过时（现分派远超九路）。同时 **env 家族（7 个 WALKTHROUGH_* env）无汇总注释**——散落各定义处，可发现性差。

纠偏（纯注释，零行为改动）：
1. 分派表补全 4 漏记分支+修正「其余」描述+persona 措辞对齐（修稿编辑三形态/审稿 JSON 注入）。
2. 新增 **env 家族汇总注释块**（7 个 WALKTHROUGH_* env 一表：FAIL_ARCHITECT/STREAM_DELAY_MS/AUDIT_SCORES/AUDIT_SCOPE/REVISE_BLOAT/REVISE_PASSTHROUGH/REVIEW_RETRIES——各自驱动的行为与出处号）。
3. 表头加 657 教训锚（「新增/改动分派须同步本表」）。

## 二、pi1.0 key 复探（664 复核结论维持）

repo 根 `.inkos/secrets.json` 不存在、环境无 `*_API_KEY`——key 未就位，L6/L1 维持挂起（provider-smoke 已备）。

## 三、验证

- node --check + review-matrix 套件全绿（五场景 28 断言——注释改动零行为验证）。
- gate:ts 八步全绿（HEAD 复验）。
- Rust 零触碰（654/658 矩阵锚定引用链有效）。

## 四、教训

657 教训的落地形态：对齐审计不是一次性动作——审计结果（对齐后的分派表+env 家族汇总）要**写回头部注释**成为后续改动的对照基线，否则审计知识只留在变更记录里，下一次改动仍凭记忆。
