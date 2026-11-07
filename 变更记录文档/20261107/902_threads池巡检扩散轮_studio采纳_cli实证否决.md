# 902 号 · threads 池巡检扩散轮：studio 采纳（-21~24%）+ cli 实证否决（chdir worker 硬阻断）

> 901 备案「studio/cli 留默认 forks 备案观察（下一巡检轮可选）」本轮
> 清偿：同款三线实验矩阵在两包面分别取证。studio 采纳（threads 2×
> 0 败 38.24/38.21s + 配置生效形态 40.12s，vs forks 50.45/48.55s）；
> cli 否决（threads 6 败 2×稳定复现，单一根因 process.chdir() is not
> supported in workers，收益微 3.8→3.5s）。验证：gate:ts 八步全绿
> （test 步 112.8→112.9→106.2→**97.5s** 序列）、clippy 双 0、
> testgate 2044+584、U+FFFD 净。

## 一、实验矩阵（同口径 `pnpm vitest run` 全量，JSON reporter）

| 包 | 线 | 跑 1 | 跑 2 | 配置生效形态 | 绿否 |
|---|---|---|---|---|---|
| studio (141 文件/755 用例) | forks 基线 | 50.45s / 0 败 | 48.55s / 0 败 | — | 稳定绿 |
| studio | threads | 38.24s / 0 败 | 38.21s / 0 败 | 40.12s / 0 败 | **3×绿，-21~24%** |
| cli (23 文件/81 用例) | forks 基线 | 3.85s / 0 败 | 3.73s / 0 败 | — | 稳定绿 |
| cli | threads | 3.53s / **6 败** | 3.47s / **6 败** | — | 大红稳定复现 |

studio threads 收益绝对值 ~10-12s（38 vs 49），占比 -21~24%——比 core
（-13~16%）更大的原因：fileParallelism:false 串行面下 fork 开销占比更
高，且 jsdom/react 组件测试模块图更重。

## 二、cli 否决定案（单一根因，不属缺陷）

6 败全部为 `tui-command.test.ts`（15 行 `process.chdir(...)`）：
`TypeError: process.chdir() is not supported in workers`——
worker_threads 硬 API 限制，2×稳定复现非时序。cli 收益本就微
（3.8→3.5s，-6%），改造 chdir 用法（换 spawn cwd 参数形态）属无关
重构，范围纪律不做。cli 留 forks 恒定；备案：若未来 cli 面测试量
级增长可重新评估（届时先收 chdir 站）。

## 三、落地

`packages/studio/vitest.config.ts` test 块增 `pool: "threads"`+证据注
释（含 cli 否决备案「forks 留守，勿顺手改 cli」）。workspace 混合形态
（core threads + studio threads + cli forks）由 gate:ts test 步全量
验证：**112.8→112.9→106.2→97.5s** 四轮连绿序列——901+902 两轮 threads
扩散合计兑现 ~15s/轮门禁耗时。

## 四、验证汇总

- studio threads 取证 3×755 用例绿；gate:ts 八步全绿（test 97.5s）。
- clippy:gate 双 0；cargo:testgate 2044+584（零 Rust 改动按纪律复跑）。
- U+FFFD：vitest.config.ts 净。

## 五、后续出口

- threads 扩散至此面饱和（core+studio 已采纳，cli 实证阻断）。
- 在册出口清空，回归常备：上游协议扩展或外部事件（pi1.0 key）或新
  真实需求或下一巡检周期。

**下一号自 903 起；出口清空回归常备；用户侧待办=Fork 推送 36 个提交。**
