# 415 · doctor 诊断面双端字段级对照（轻量纯复核，零缺陷结论）

日期：2026-09-14　性质：轻量质量面复核（高频排障入口）　复核对象：GET /api/v1/doctor（195/349/381 号累积面）

## 对照结论（代码级字段清单比对）

Rust `ops_routes::get_doctor` 与 TS server `/api/v1/doctor` 的 checks 供给
**九项字段级对齐**：inkosJson / projectEnv / globalEnv / booksDir /
llmConnected（probe 主干）/ bookCount / bookIssues（state-degraded 前置
预警，195 号）/ retrieval{ mode, embeddingConfigured, vectorEngine,
vecExtensionAvailable, chunkCount }（349/380/381 号累积）。

消费端验证（281/286 纪律）：`DoctorView.tsx` 读取的字段
（llmConnected/bookIssues/inkosJson/projectEnv/globalEnv/booksDir/bookCount/
retrieval）全部在双端供给集内，无「前端读、后端缺」字段。

## 结论

doctor 诊断面零缺陷，双端对齐。零代码变更，纯复核记录。

## 附带增量说明

本批复核期间确认用户已在 Fork 推送部分批次（origin/develop=e555d6f3）；
剩余积压（402–414 及并行多批）请继续在 Fork 图形端推送，核验
origin/develop 追平本地 develop（最新提交）。
