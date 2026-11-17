# 1490 号：picomatch 公告族扫复核轮——glob/ReDoS 族全覆盖定谳（零代码改动）

## 目标与范围

/goal 自主持续开发续轮。1489 picomatch 公告处置后的「修洞按族扫」
纪律复核（846/848 同族扫先例）：glob/ReDoS 族成员处置完整性+
audit:npm 扫描范围完整性。

## 族扫结果（全绿：1489 处置完备）

①**族成员存在性**（根 node_modules/.pnpm——studio 局部查 0 为
hoist 假象，复核纪律：pnpm workspace 依赖位置先定再查）：
- **picomatch 双版本在树**：4.0.7（vite→fdir/tinyglobby/shadcn→
  dotenvx 链）+**3.0.2**（fdir 6.5.0 的 optional peer ^3.0.2+
  micromatch 4.0.8 依赖）——**双版本同在 GHSA-fc5q-39xc-vjpf
  漏洞区间（≤4.0.7）**，白名单按模块+id 匹配天然覆盖双版本；
  3.0.2 消费链定性复核=同样全 dev 工具链+自家源码面+运行时零
  执行，1489 定性对双版本成立 ✓；
- **micromatch 4.0.8**（最新线）：audit 报告面零命中 ✓；
- **braces 3.0.3**（=latest）：零命中（858 定谳维持）✓；
- extglob/snapdragon/fill-range/is-glob：不在树 ✓。

②**audit:npm 扫描范围完整性**：脚本在仓库根对单一根
pnpm-lock.yaml（workspace 统一锁）跑 `pnpm audit --json`——
**全 workspace 覆盖**（studio/cli/core 包无独立锁，无漏扫面）；
当前报告面 advisory 合计 5 条全白名单放行 0 拦截。

## 结论

1489 picomatch 处置按族复核**完备**：同族成员（micromatch/braces/
extglob）无未处置命中；picomatch 自身双版本均被 id 钉定白名单
覆盖且定性成立；扫描范围全 workspace 无死角。

零代码改动零新缺陷，U+FFFD 净。

出口=下一巡检周期（约 30 天后，2026-12 中旬前后到期）/earendil-mz
单包合并；用户侧待办=**Fork 推送（含 955-1490，Fork 图形端操作）+
正式打包三脚本重烤（必须含 engine-rs release 重编——1486 依赖
刷新+1487 fmt 后重编必要性加码）**。下一号自 1491 起。
