# 434 号：pnpm 死覆写迁移——package.json pnpm 字段清退入 workspace.yaml

日期：2026-09-14　性质：真实缺陷修复（工程面）

## 缺陷

433 号体检中发现每次 pnpm 命令刷 WARN：`The "pnpm" field in package.json is no
longer read`。根因：pnpm 11 起依赖覆写迁移至 pnpm-workspace.yaml（277/278 号已迁
安全覆写批），但 `package.json` 残留 `pnpm.overrides` 两条
（`@mariozechner/pi-ai: 0.67.1`、`@mariozechner/pi-agent-core: 0.67.1`）——
**已被静默忽略的死配置**，原意"全图强制 0.67.1"实际只剩直接依赖精确规格承载。

## 核查

- 传递链现状：`node_modules/.pnpm` 仅 `pi-ai@0.67.1` / `pi-agent-core@0.67.1`
  双拷贝各 2（peer 组合差异），无版本漂移——直接依赖 `0.67.1` 精确规格暂扛住了钉版；
- 429 号安全覆写（brace-expansion 等）在 workspace.yaml 正常生效（brace-expansion@2.1.4、
  vite@7.3.5 均已落盘），**未受影响**。

## 修复

- `package.json`：删除整个死 `pnpm` 字段（WARN 消除）；
- `pnpm-workspace.yaml`：两条 pi 系覆写迁入 `overrides` 并注释来历——兜住传递链，
  杜绝未来新增传递依赖时出现第二版本拷贝；
- `pnpm install --registry=https://registry.npmjs.org/` 同步 lockfile
  （npmmirror 镜像 type-fest@5.9.0 滞后导致默认源解析失败，与本次改动无关）。

## 验证

- `pnpm --version`：WARN 消失；
- `pnpm-lock.yaml` diff 恰好 +2 行（两条覆写记录），`downloaded 0, added 0`
  ——解析结果零变动；
- `pnpm --filter ./packages/core typecheck`：tsc 通过。

## 关联

package.json 同时承载 433 号的 `clippy:gate` 挂载行（相邻 hunk），随本笔一并入库。
