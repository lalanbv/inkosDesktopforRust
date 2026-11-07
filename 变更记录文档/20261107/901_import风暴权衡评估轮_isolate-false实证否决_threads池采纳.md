# 901 号 · import 风暴权衡评估轮：isolate:false 实证否决 + threads 池采纳

> 898 备案的最后一个权衡面本轮以实验矩阵清偿。三线对照：基线
> forks+isolate（2×0 败 56.42/57.57s）、forks+no-isolate（47→67 败级
> 联放大 40.47s→143.46s，含 mock 泄漏→**真实外网调用**=硬否决证据）、
> threads+isolate（2×0 败 48.11/50.34s + 配置落地形态 51.48s=3/3 绿）。
> 裁决：isolate:false 否决；threads 采纳（core only，studio/cli 留
> forks 备案观察）。验证：全量 3×1059 绿、gate:ts 八步全绿（test 步
> 112.8→106.2s）、clippy 双 0、testgate 2044+584、U+FFFD 净。

## 一、问题面（898 备案原文回放）

「import 风暴（929s/48% 墙钟）：isolate:false 是大杠杆但有共享模块状
态风险，另轮评估。」——929s/48% 是尸挂扭曲数字；898 健康基线实测
import 占比健康画像 12%。本轮在**健康基线**下重新定量并实验裁决。

## 二、健康基线画像（JSON reporter，文件级 worker 占用）

- 墙钟 57.57s/56.42s（2×，与 898 实测 55.44s 同族）；185 文件 worker
  占用总和 85.0s（maxWorkers:2 下墙钟下限≈42.5s，57.57 合理）。
- **Top10 文件占 68.4%**：short-revision.flow 18.2s、pi-stream 13.6s、
  longform-harness 4.6s……头部是执行重（真计时器/真 I/O）非 import 重
  ——「import 风暴」的摊薄空间被头部执行成本封顶，理论收益上限有限。

## 三、共享模块状态风险面普查（定量）

| 风险面 | 站数 | 定性 |
|---|---|---|
| `vi.mock(` 使用文件 | 8 | 共享注册表下 mock 注册跨文件泄漏面 |
| `process.env.X =` 直接赋值（非 stubEnv） | 17 站 | env 变异跨文件泄漏面（unstubAllEnvs 仅 7 文件成对） |
| createServer / closeAllConnections | 25 : 25 | **全配对**——服务器句柄面卫生 |
| globalThis.fetch 重装 | 1 文件（probe.test.ts） | 有保存/恢复 origFetch——好形态 |
| useFakeTimers | 1 文件 | 单站，面窄 |

## 四、实验矩阵（三线，同口径 `pnpm vitest run` 全量）

| 线 | 形态 | 跑 1 | 跑 2 | 绿否 |
|---|---|---|---|---|
| A | forks+isolate（基线） | 57.57s / 0 败 | 56.42s / 0 败 | 稳定绿 |
| B | forks+`--no-isolate` | 40.47s / **47 败** | 143.46s / **67 败** | 大红且级联放大 |
| C | `--pool=threads`（isolate 保持 true） | 48.11s / 0 败（1059） | 50.34s / 0 败（1059） | 稳定绿 |

B 线 47 败五类形态全部对上 §三普查面：①「Body is unusable: Body has
already been read」~20 败（模块级缓存 Response 跨文件复用）；②
llm-retry 4 败——**mock 注册被覆盖 → 真实 fetch 打到 api.minimaxi.com
（401）**=测试沙箱性破坏，硬否决级证据；③ provider-minimax 6 败
（response.json is not a function / Streaming body unavailable=异文件
mock 形态泄漏）；④ proxy-fetch vi.fn 断言不匹配（模块级 agent 缓存）；
⑤ translation-ingest unpdf PDF.js worker 全局态。B 线耗时也不稳定：
40.5s→143.5s（失败互作级联把名义收益吃掉）。

## 五、裁决

1. **isolate:false = 否决**。三重证据：失败面 47→67 级联放大、五类失
   败形态与普查风险面一一对应、mock 泄漏产生真实外网调用（可重复性
   与沙箱性双重破坏——测试跑门禁若真打 LLM 端点既污染计费又不可重现）。
2. **threads 池 = 采纳（core only）**。收益机制=省每文件 fork 开销，
   isolate:true 语义不变（每文件独立模块注册表，vi.mock/env/服务器句
   柄隔离面与 forks 同级——§三风险面全部不激活）。范围纪律=证据到哪
   改到哪：threads 实验只在 core 面取证，studio/cli 留默认 forks 备案
   观察（它们量小 141/81 用例，收益有限；下一巡检轮可选）。
3. 落地：`packages/core/vitest.config.ts` 增 `pool: "threads"`+证据注
   释；配置生效形态复跑 51.48s / 0 败（与 CLI 形态一致）。

## 六、验证汇总

- 全量 3×1059 用例绿（48.11/50.34/51.48s）；gate:ts 八步全绿，test 步
  **112.8→106.2s**（threads 收益在 workspace 递归面兑现，三轮连绿）。
- clippy:gate 双 0；cargo:testgate 2044+584（零 Rust 改动按纪律复跑）。
- U+FFFD：vitest.config.ts 净。

## 七、后续出口

- studio/cli threads 采纳观察（下一巡检轮可选，非义务）。
- env 直接赋值 17 站卫生债备案（isolate:false 否决后非必须，stubEnv
  成对化为卫生项）。
- **在册出口清单清空**：898（SSRF 链）→899（族扫）→900（SSRF 缓解）
  →901（isolate 权衡）备案链全部清偿。剩余出口回归常备=上游协议扩展
  或外部事件（pi1.0 key）或新真实需求或下一巡检周期。

**下一号自 902 起；出口清空回归常备；用户侧待办=Fork 推送 35 个提交。**
