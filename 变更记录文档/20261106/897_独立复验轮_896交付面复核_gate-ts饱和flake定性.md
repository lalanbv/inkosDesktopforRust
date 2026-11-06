# 897 号 · 独立复验轮：896 交付面复核（coverRequired 修复+全路由冒烟）+ gate:ts test 步饱和 flake 定性

> 896 号（coverRequired 优先级修复+生产全路由冒烟）由并行会话收口落库
> （df0a9479，文档 896_验收轮_coverRequired优先级修复_全路由冒烟_895面验收.md）。
> 本号对该交付面做**独立复验**（TS L427 逐字对照+修洞按族+冒烟真跑+门禁
> 电池全绿），并备案验收中发现 gate:ts test 步饱和型 flake（三跑证据链，
> 896 提交面未载此况）。
> 复验：冒烟绿（INKOS_DUEL=1）、short78 族 5/5、testgate 2040+584、clippy
> 双 0、U+FFFD 净；gate:ts 七步绿 + test 步 flake 定性（§四）。

## 一、撞号改号备案

- 本轮原以 896 号起草（修复+冒烟+flake 备案一体）。收口前查并行落库发现
  896 已被并行会话占用并提交（df0a9479，含同题修复三件+冒烟器械+e2e 双
  形态断言——本会话采纳其在途工作续跑验证后由其收口落库）。
- 按「撞号让号改号」（534/536/541/540 先例）让出 896、本号改 897；修复
  本体归 896 提交面，本号重定位为独立复验+flake 定性。

## 二、896 交付面独立复验（全绿）

### 修复本体对照（TS L427 逐字）

- `ShortFictionRunOptions.cover_required: Option<bool>` 分字段（`Some(false)`
  ≠ `None`）↔ TS `options.coverRequired` 2.0 显式字段，与全链 legacy
  `cover` 分离。
- `produce_short` 优先链 TS L427 逐字：`cover_required.or_else(默认链)`——
  `Some(false)` 显式保持压过 previous 继承面；`None` 才落默认链
  （stopAfter/revision 分支→previous 继承，其余→`cover.unwrap_or(true)`）。
- 路由 `coverRequired` 参数改接 `cover_required`（production 门语义不变：
  outline/draft 才读产出参数）；revise 路径显式 `cover_required: None`
  （fall-through 继承=TS L842 同语义，`cover: Some(false)` 不动）。

### 族扫（修洞按族——846 纪律）

TS 四个 cover 触点 ↔ Rust 逐点对齐确认：L257 生成门（`cover !== false` ↔
runner `unwrap_or(true)`）、L315 stage 入口 `cover:false`（↔ runner:620，
stopAfter 分支下默认链不可达、`cover_required` 经 clone 透传）、L427 优先链
（↔ runner:1122）、L842 revise `cover:false`（↔ runner:3499/4020）。agent
确认流入口 `agent_production.rs:1847` 落 legacy `cover`——两入口分字段不混，
闭环无同族漏网。

### 冒烟真跑（证据轮纪律）

`INKOS_DUEL=1 cargo test --test short_capability_router_smoke`：1 绿 1.65s，
mock 19 轮按序耗尽（capability 四动作全流+revise scoped 授权链+错误三面零
LLM 轮+agent short_run 桌面确认流+L1 读面，mismatch 弹即失=顺序即契约）。
缺 env 早退计 testgate 2040（不进基线，duel-requires-inkos-duel-env 同款）。

### e2e 与电池

- short78 族 5/5：coverRequired true/false 双形态断言在位（false 显式形态=
  本轮缺陷的直接回归锚）；895 参数契约组回归绿。
- testgate 2040+584（51 目标，冒烟入电池）；clippy:gate 双 crate 0；
  改动文件 U+FFFD 扫描净。

## 三、gate:ts test 步饱和型 flake（三跑证据链，896 提交面未载）

**现象**：core 包全量 vitest 三跑三败、失败面漂移——run1（与 cargo
testgate 并行）：`short-source.flow` 20s 超时；run2（空闲复跑）：同测试再
超时；run3（空闲）：换 `longform-harness.flow` resume（20s）+
`short-revision.flow` finalize（30s 撞全局 testTimeout）两枚。三枚单跑全绿
且余量 13~20 倍（457ms / 1511ms / 文件 15.2s）。

**定性**：真计时器+真 I/O 流测试在 maxWorkers:2 饱和调度下的尾延迟超预算
（core vitest.config 自述已知此风险族；失败面随共调度重文件漂移=饱和型非
确定性污染）。TS 自 895 零改动，非 896 回归。

**与 896 提交面出入备案**：df0a9479 提交文载「gate:ts 八步」全绿——本会话
三跑均 test 步红（概率性饱和，非确定性红）。两侧证据并存备案；flake 属概
率触发，不据此判 896 复验不成立，但**门禁红绿确定性已破**，以本证据链为准
作 898 号专轮输入。

**失败面收敛佐证**：studio 全量 vitest exit 0（141 文件 / 755 用例全绿，
990.31s）；cli 23 文件 / 81 用例绿——三跑失败全部落在 core 包。

## 四、898 号专轮输入（flake 恢复确定性红绿）

套件内逐测试计时画像 → 定位共调度重文件 → 重预算（流测试对齐 60s）或拆
分重文件/削减真 I/O。原则：不洗失败、让门禁恢复确定性红绿（对齐 846 门禁
纪律——恒绿与概率红同属门禁失真）。

**下一号自 898 起：TS 流测试饱和 flake 专轮；出口不变；用户侧待办=Fork
推送 31 个提交。**
