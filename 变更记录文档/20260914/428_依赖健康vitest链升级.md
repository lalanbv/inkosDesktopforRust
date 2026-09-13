# 428 · 依赖健康专项——vitest 内嵌 vite 7.3.1 advisory 修复

日期：2026-09-14　性质：依赖健康专项批（427 号备案执行）

## 内容

`pnpm-workspace.yaml` overrides 新增 `"vite@7.3.1": 7.3.5`——把 vitest 3.2.7
链内嵌的 vite 从 7.3.1（`server.fs.deny` bypass 与任意文件读 advisory 区间
>=7.0.0 <=7.3.4）钉到修复版 7.3.5。精确版本选择器不触及 studio 自身的
vite 6.4.3（跨主版本零风险）。

## 验证

- audit：31 → **26 条**（high 16→13，moderate 11→9）——vite 相关 5 条全部消除；
- lockfile：`Packages: +2 -2`（vite 7.3.1 双实例替换为 7.3.5）；
- 回归：core 245 文件 2112 用例全绿、studio 97 文件 823 用例全绿、双包
  tsc 净（vitest 运行器内嵌 vite 变化零影响）；
- install 需官方源（npmmirror 缺 type-fest@5.9.0，411 号同款 registry 教训）。

## 遗留

- audit 剩余 26 条为 basic-ftp / picomatch / brace-expansion 等 ai 链
  传递依赖锁定——升级 ai 链影响面大，继续备案（依赖健康专项后续）。

## 推送提醒

origin/develop=e555d6f3；剩余积压（402–428，含 7+ 真实缺陷修复 + 本次
安全修复）请在 Fork 图形端推送，核验 origin/develop 追平本地 develop
（428 提交）。
