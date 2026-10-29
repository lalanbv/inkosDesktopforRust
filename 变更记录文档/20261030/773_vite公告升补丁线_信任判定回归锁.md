# 773 号：vite 新公告 override 升补丁线（server.fs.deny 绕过）+ 信任判定谓词回归锁 + 文档对齐

> 本批三件：772 锚定后的新公告清偿（758 号方法论第二次实战）、769 信任
> UX 的回归锁补齐、docs 对齐收尾（均为零行为风险面）。

## 一、vite 公告清偿（GHSA-8g5g-mxqh-grgj，moderate）

- **归因**：公告库漂移（本仓 vite 依赖零触碰；`git log -- package.json
  pnpm-lock.yaml` 上一触碰为 758 号）。
- **公告本体**：`server.fs.deny` 可被 query 绕过；vulnerable `=6.4.4`、
  patched `>=6.4.5`（官方 registry 实取）——**同 major 有补丁线**，
  走 758 号 override 升补丁线处置（非白名单）。
- **暴露面定性**：vite 为 studio 构建工具链（dev/构建期），生产产物
  不含 vite server；即便 dev 期，`server.fs.deny` 绕过要求攻击者可控
  请求打到 dev server（本机单用户桌面形态）。
- **处置**：根 override `vite@<7: ^6.4.3 → ^6.4.5`；lock 实达 6.4.5
  唯一解析；audit 回绿（10→9 advisory，拦截 0）。
- 插曲：pnpm install 打印 fdir/picomatch unmet peer 警告——既有依赖图
  形态非本次引入（gate:ts build/test 全绿反证无新破坏）。

## 二、信任判定谓词回归锁（769 号测试债补齐）

- 内联 JSX 判定抽为纯函数 `pendingTrustSourcePath`（`skill-ui-state.ts`，
  与 normalizeSkillId 等既有导出同域）：载入闸诊断串 + 非空 path →
  可确认路径；普通格式诊断/缺 path/空 path → null。
- 新增 4 测（skill-ui-state.test.ts）：命中/普通诊断/缺 path/缺 message。
- ProjectSettings.tsx 改消费该函数（行为零变化，tsc 干净）。

## 三、docs 对齐（772 号续，已随 docs 勘误提交入库）

- USER_GUIDE/QUICK_START 补外部来源信任说明（3df24470）。

## 四、验证

- skill-ui-state 9 passed（+4）；page-mount-smoke-3 7/7；gate:ts 八步
  全绿（audit:npm 复绿后全过；vite 6.4.5 下 build/test/typecheck/
  node-fallback-smoke/export-epub-smoke 全过=升级无回归实证）。
- 契约面零触碰（engine-contract-diff 0 分歧）；bench 面无需重锚。
