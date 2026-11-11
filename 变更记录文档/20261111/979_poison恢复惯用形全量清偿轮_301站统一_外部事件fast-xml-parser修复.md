# 979 号 · poison 恢复惯用形全量清偿轮：301 站统一 + 外部事件 fast-xml-parser 修复

> 常备出口「自主勘察工作面」选题：生产面 panic 源普查（goal「最安全可靠」直系）→
> 聚焦 **lock poison 族全量清偿**。301 站/50 文件统一为 poison 恢复惯用形
> `.unwrap_or_else(|poisoned| poisoned.into_inner())`（仓内既有先例
> backup_routes.rs:448 惯用形确立）；全站统一含测试面（757/779 原子写收敛先例，
> 免行级 cfg(test) 区分难题——普查脚本 brace 漂移实证不可靠）。
> 附加外部事件：audit:npm 抓到 advisory-db 当日新增 GHSA-474x-hrwr-fgx4
> （fast-xml-parser [low] Quadratic CPU scanning），修洞优先升 5.12.0 复绿。
> 门禁：clippy 双 0 / testgate **2111(+1)+584** / audit:rust 双 0 自检 7+1 活 /
> audit:npm 绿四夹具活 / gate:ts 十步全绿 / U+FFFD 净。

## 一、普查面（panic 源全景与分型）

- 双 crate 生产面初盘（python 状态机区分 cfg(test) 区块）：engine-rs 669 站
  （unwrap 429+expect 231+panic 2+unreach 7）/ src-tauri 122 站（unwrap 75+
  expect 44+panic 1+unreach 2）；测试面 2575 站不计债（测试 unwrap=惯例）。
- **普查方法论边界**：状态机跟踪 cfg(test) mod 有 brace 漂移误差（字符串内
  `{}` 计入 depth——panic! 台账 19 站人工复核=13 站测试面+2 站生产 unreachable
  （play_graph.rs:336/355，前置 `matches!` 守卫的不可能分支）+4 站注释）；
  逐行精确归属需真 parser——本轮改用**形态级人工复核**粒度。
- **分型**（grep 精确+python 多行正则）：`.lock().unwrap()` 220 单行+46 分行；
  `.lock().expect("…")` 27 单行+6 分行；RwLock `.read()/.write().unwrap()` 2
  （static_routes.rs:119/125，io 误伤排查=全仓仅此两站）；io 面 read/write
  unwrap 0 站混入。**合计 301 站**。
- 余量备案：非 lock 族生产面 unwrap/expect ≈441 站（engine 161+198 /
  tauri 59+23）——后续轮分批（前置不变量 expect 与解析面裸 unwrap 需逐站
  人工定性，机械替换不适用）。

## 二、替换面与语义

- 统一形：`.lock()/.read()/.write()` 的 `unwrap()/expect("…")` →
  `.unwrap_or_else(|poisoned| poisoned.into_inner())`——语义=poison 后恢复
  （锁内数据可用性优先，std poison 仅标记持锁线程 panic），与仓内先例
  backup_routes.rs:448 惯用形一致；866 号反模式族扫方法论适用（族扫+先例定性）。
- **全站统一含测试面**（757/779 先例）：逐站行级区分生产/测试依赖不可靠的
  cfg(test) 状态机（brace 漂移实证）——全站统一既免难题又收测试面一致性；
  测试内少数故意制造 poison 的裸 unwrap（探针形）不受影响（替换只动
  unwrap_or_else 目标面，探针后写）。
- 执行：python 多行正则（单行+分行形态一并覆盖——grep 单行扫漏 46 站分行形）
  → 双 crate `cargo check --all-targets` 过 → 残余 0（grep 复查）。
- panic!/unreachable! 定性：play_graph 2 站保留（守卫不可能分支）；其余全为
  测试断言/注释（人工复核实证）。

## 三、poison 恢复测试（新例）

- `agent_route.rs` tests 新例 `pending_session_aborts_registry_survives_poison`：
  探针线程持锁 panic（制造 poison）→ `mark_pending_session_abort` 恢复形
  insert 成功 → `take_pending_session_abort` 受理裁决命中且取走后不重复命中。
  裸 unwrap 形（979 前）此处 panic → 本例红=恢复形存在性测试钉。
- 选站理由：mark_pending_session_abort 为 pub、同步、双参轻函数，内部锁站
  恰为本轮替换形；tests 模块显式 use（553 号显式导入先例，首版 glob 缺失
  E0425 编译红抓出补导）。

## 四、必红两形

- **NEUTER979 env 形**：mark_pending_session_abort 内 env 分支回退裸 unwrap →
  `NEUTER979=1` 跑 poison 测试**恰红 1**（本例 panic）；撤除后复绿，实现面
  0 残留（grep 实证；测试注释保留中和点说明=975-977 先例惯例）。
- **stash HEAD 形**：stash 全部 50 文件 → HEAD 裸 unwrap 形 → python 重插
  测试段（同文件实现面=HEAD 形）→ 恰红 1 → `git checkout` 丢弃重插 →
  `git stash pop` 恢复 50 文件 → 复绿；stash 栈 0。
- 两形红面均=poison 测试本体（恰红非级联红，--lib 单测过滤实证）。

## 五、真引擎活体（8979 / mock 18979）

- 环境：walkthrough-mock.mjs 18979 + inkos-engine-server release 二进制重建
  （59.3s，活体前新鲜度纪律）+ INKOS_PORT/PROJECT_ROOT/LLM_BASE_URL env。
- abort 面：POST /api/v1/sessions/nonexistent-979/abort →
  `{"aborted":false,"ok":true}` + SSE 流收 `agent:aborted`——mark_pending_session_abort
  恢复形站+hub 广播链活体。
- chat 全链：建会话（chat kind）→ POST /api/v1/agent → 200 五键
  （details/response/session/timings/usage）+ mock 真分派 SECTION 产物——
  过 agent_route 主路径 registry entry or_insert/queue lock_owned await/
  abort_flag/轮末 remove/观测面快照**全部恢复形站**。
- SSE 全链事件：`agent:start → draft:delta → agent:complete → session:title`
  （+ping；首轮 SSE 抓取 -m 30 超时断流教训=重抓 -m 40 复跑）。
- 环境清偿：mock/engine/SSE 进程 0 残留+临时根删除。

## 六、外部事件：fast-xml-parser GHSA-474x-hrwr-fgx4（修洞优先）

- gate:ts 期间 audit:npm 首跑**恰红**（848 形状卫兵活体实证非误报）：
  `fast-xml-parser [low] GHSA-474x-hrwr-fgx4` Quadratic CPU consumption
  scanning unterminated stop-node tags——978 昨日跑时尚无=advisory-db 当日
  新增，真·外部事件。
- **修洞优先于白名单**（848 纪律）：直接依赖面（pnpm.overrides 强制全
  workspace）→ 5.12.0（2026-11-03 发布）=修复线，^5.7.0 semver 兼容。
- **pnpm 坑实录**：`pnpm update fast-xml-parser [--latest]` 均 "Already up
  to date"——**update 不重解析 pnpm.overrides**；破法=改 override spec
  `^5.7.0 → ^5.12.0`（package.json Edit 形——Mimosa 拦 Bash 直改）触发
  重解析 → lock 5.11.2→5.12.0 → audit:npm 复绿（6 advisory 全白名单
  0 拦截）。
- install 副产物观察（既有面备案）：unmet peer picomatch@^3.0.2 vs found
  4.0.7 警告——非本轮引入，后续轮定性。

## 七、门禁

- `clippy:gate` 双 0（301 站替换+依赖升级未带新 lint）。
- `cargo:testgate`：engine-rs 52 目标 **2111 passed**（+1=poison 新测试）+
  src-tauri 25 目标 **584 passed**（持平）——301 站替换零回归。
- `audit:rust` 双 0（警告 2/9 维持）+双夹具自检活体（7+1 RUSTSEC）。
- `audit:npm` 绿（升级后 6 advisory 全白名单）+四夹具自检活（含未白名单
  告警→红——本轮 GHSA 真红实证）。
- `gate:ts` 十步全绿：build 16.4s/typecheck 14.6s/test 101.8s（pnpm -r test
  全量——fast-xml-parser 5.12.0 升级回归面在内）/audit:npm 2.6s/rust-bin
  22.7s/node-fallback-smoke 35.9s/engine-contract-diff 36.5s/export-epub-smoke
  35.5s/work-alternation 5.2s/check-routes 0.1s。
- U+FFFD：0 命中净；pnpm outdated 复查 0 行。

## 八、备案

- 非 lock 族生产面 panic 源余量 ≈441 站分批后续轮（需逐站人工定性）。
- play_graph.rs 2 站 unreachable! 保留定性（前置守卫不可能分支）。
- picomatch peer 警告既有面观察。
- SSE 全局流首轮抓取超时断流——活体惯用形微调（-m 40+）沉淀。

**下一号自 980 起；出口回归常备；用户侧待办=Fork 推送（含 955-979）。**
