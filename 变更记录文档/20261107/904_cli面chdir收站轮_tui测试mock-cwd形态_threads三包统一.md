# 904 号 · cli 面 chdir 收站轮：tui 测试 mock-cwd 形态 + threads 三包统一

> 902 备案「cli 否决（chdir 阻断），未来量级增长先收 chdir 站再评估」
> 的提前执行：chdir 全仓普查=**唯一站** tui-command.test.ts（生产代码
> 零 chdir），改 mock cwd 读数收站（worker 下合法），threads 复测解锁
> （2×0 败 3.87/3.90s vs forks 4.10s），收益微为正零风险 → cli 落地
> `pool: "threads"`，三包（core/studio/cli）配置面统一。
> 验证：cli 改后 forks 0 败无回归+threads 2×0 败+配置生效形态 4.19s/
> 81 全绿+零进程残留；gate:ts 八步全绿（数字回填提交信息）。

## 一、修洞面（按族普查定窄）

`process.chdir` 全仓普查（packages/{cli,studio,core}/src）：**唯一站**
= `tui-command.test.ts` beforeEach（生产代码 5 站全是 `process.cwd()`
读取——utils.ts/program.ts/init.ts/tui.ts/api 入口，无 chdir 写面）。
902 的 6 败全部命中 beforeEach chdir 行。

## 二、收站形态（三选一的最小侵入解）

- A spawn 子进程形态（真 cwd，最真实但慢+新 harness）；B 依赖注入
  （program 加 cwd 参数，侵入生产三站）；**C mock cwd 读数（采用）**：
  `vi.spyOn(process, "cwd").mockReturnValue(tempDir)` 替代 chdir——
  worker_threads 下完全合法（mock JS 层读数，无 OS 限制），被测链
  （init/tui 命令经 process.cwd() 取目录）同语境等价，afterEach 既有
  `restoreAllMocks` 复原零新增。断言面 `process.cwd()` → `tempDir`
  直引。`process.chdir` 的 OS 级限制（worker 硬禁）与 mock 层等价性
  是本解成立的根据。

## 三、threads 解锁与落地

- 改后 forks 基线 4.10s / 0 败（改动无回归）；threads 2×**0 败**
  3.87/3.90s（902 的 6 败全消=解锁实证）。
- 收益微为正（~-5%）但稳定零风险（隔离语义不变，core/studio 已证稳），
  落地 `packages/cli/vitest.config.ts` `pool: "threads"`+证据注释——
  三包配置面统一（心智一致，未来 cli 量级增长自动兑现收益）。
- 配置生效形态 4.19s / 81 全绿；进程残留核对 0（903 清场机制无泄漏）。

## 四、验证汇总

- cli：改后 forks 0 败 + threads 2×0 败 + 配置生效 0 败（4 跑全绿）。
- gate:ts 八步全绿（test 100.1s；engine-contract-diff 步 586.4s 为
  与前台复测负载并存的满载瞬态——进程表净+单步隔离复跑 36.72s 回
  基线，功能判定绿有效，按 903「先查进程表+隔离复跑」纪律处置）。
- clippy:gate 双 0；cargo:testgate 2044+584（零 Rust 改动按纪律复跑）。
- U+FFFD：三改动文件净。

## 五、后续出口

- threads 三包统一完成，扩散面终局饱和。
- 在册出口清空，回归常备：上游协议扩展或外部事件（pi1.0 key）或
  新真实需求或下一巡检周期。

**下一号自 905 起；出口清空回归常备；用户侧待办=Fork 推送 38 个提交。**
