# 501 号：豁免表复核——skills/genres 镜像缺口实为环境伪象，双端活体等价证实

日期：2026-09-16　分支：develop　基线：614bfb21（500 号）

## 复核过程

488 号曾将 `/skills`（16 vs 1）与 `/genres`（15 vs 2）豁免为"内置包镜像缺口"。本号复核发现差分器 rust 腿只注入了 `INKOS_BUILTIN_GENRES_DIR`、**从未注入 `INKOS_BUILTIN_SKILLS_DIR`**，且 genres env 注入后即被豁免遮住、从未复验。本号：rust 腿 env 补 `INKOS_BUILTIN_SKILLS_DIR = packages/core/skills`、移除两条豁免，活体重跑。

## 结论：双端内置面活体等价

- `/skills` 与 `/genres` 双端一致（exit 0，35 断言 0 分歧）；
- **488 号"skills 16 vs 1 移植缺口"重新定性：非产品缺口，是 harness 环境伪象**——引擎 builtin 面完整，缺的只是 env 接线；桌面壳已在 489 号修复（resolve_builtin_asset_dirs + 打包补资源）；
- 原豁免条目删除，改为 env 接线注释；"skills/genres 移植评估"排队项**注销**。

## 门禁

- 差分器：**35 断言（8 写入 + 1 错误面 + 26 读取）0 分歧，exit 0**；
- 本号零产品代码改动（纯差分器 env/豁免表修正）；全量 `pnpm -r test` 3237 全绿状态延续（本日未再触发回归）；
- cargo 链接仍被 Xcode 许可阻断（481 号欠账 + 489/500 号单测运行持续排队）。

## 改动

- `scripts/engine-contract-diff.mjs`：rust 腿 env 补 `INKOS_BUILTIN_SKILLS_DIR`；豁免表删除 skills/genres 两条。

## 后续

- Xcode 许可解除后：481 号全量 cargo test（含 489/500 号 4 个新单测）、duel、bench:gate、resync Rust 侧残余评估；
- 三库种子 canonical 化待产品拍板；npm 余 2 条上游钉死项滚动跟踪。
