# 454 号：性能专项复核——bench:gate 全绿（441–453 触及路径零回退）+ npm devDeps audit 备案

- **日期**：2026-09-15
- **类型**：test(perf) —— 性能专项复核循环（零代码变更）
- **关联**：441–453 号（触及写作链/请求路径的全部改动）、257 号（负载前置检查）、428/429 号（audit 治理先例）
- **提交**：仅本记录（零代码变更）

## 1. bench:gate 全绿（441–453 累积改动性能复核）

凌晨静默窗跑 `node scripts/bench-gate.mjs`（基线 2026-09-06，阈值 +30%），9 基准全部带内：

| 基准 | 漂移 |
| --- | --- |
| chapter_index/parse_200 | -2.8% |
| paragraph_scan/detect_shape_3000zh | +2.1% |
| sensitive_words/scan_chapter | +1.3% |
| sensitive_words/scan_chapter_builtin_only | +2.0% |
| sse_broadcast/dispatch/1 | -9.4% |
| sse_broadcast/dispatch/32 | +0.8% |
| sse_broadcast/dispatch/8 | +2.1% |
| title_dedup/resolve_duplicate_hit_200 | -2.2% |
| title_dedup/resolve_no_duplicate_200 | -3.5% |

**覆盖判定**：441–453 触及的性能敏感路径——write-next 链预算估算（441 extract/derive 为 O(1) 查表+单次 stat）、API no-store 中间件（445，每响应一次 header insert）、静态面 mtime stat（449，SPA 回退每请求一次 stat，bench 不含静态面但量级为微秒级 syscall）、备份双端点（443，低频运维操作）——均无基准覆盖面回退。sse_broadcast -9.4% 为既往已知的噪声量级（433 号同款漂移定性）。

## 2. npm devDeps audit 备案（451 新增依赖复核）

`pnpm audit --registry=https://registry.npmjs.org/ --dev`：4 条告警（2 low / 2 moderate）——vitest/@vitest/mocker（moderate，mocker 路径穿越）、esbuild（low，dev server 任意文件读）、@babel/core（low，sourceMappingURL）。

定性：**全部为 dev 工具链告警，不进产物**；451 新增的 jsdom/@testing-library 系零告警；vitest 已是 3.x 最新（3.2.7），mocker 告警在 3.x 线无补丁（4.x 为大版本迁移，工程债记录）。**接受风险备案**，npmmirror registry 不支持 audit 端点须显式 `--registry`（备案到脚本化候选）。

## 3. 遗留

- **待推送：433–454 共 22 笔**，请用户在 Fork 图形端推送后核验 origin/develop。
- 451 交互基建扩展（Codex/SeriesCanon 已于 452 完成）；剩余表单面板可按需跟进。
- 下一个编号自 **455** 起。
