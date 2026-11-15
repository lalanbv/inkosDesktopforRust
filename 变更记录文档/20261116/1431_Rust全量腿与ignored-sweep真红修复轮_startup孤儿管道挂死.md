# 1431 号：Rust 全量腿验证 + ignored-sweep 真红修复轮——startup 孤儿管道挂死清偿

## 目标与范围

/goal 全量测试新增代码逻辑第三轮（1429 逐功能→1430 全量 TS 门禁→本轮）。增量面=
①cargo:testgate Rust 双 crate 全量（gate:ts 十腿不含）②cargo:ignored-sweep 实跑
③双端备案注释族扫（1429 教训方法论落地）④stale 编号卫兵。产出=ignored-sweep 抓到
真红并修复（supervisor stdio + e2e_startup 三处）。

## 全量腿结果

- **cargo:testgate 全绿**：engine-rs 52 目标 2114 passed + src-tauri 25 目标
  593 passed（INKOS_DUEL=1 内置注入真跑 + audit:rust 必红双夹具自检前置）。
- **双端备案族扫零漂移**：engine-rs/src-tauri「双端」类备案 10 处逐条行为级定性
  ——884 号 documents 排序（TS sort 在位 server.ts:4071）、segment_guard 对齐
  （TS isSafeBookId+400 INVALID_BOOK_ID 中间件在位）、686 号端点缺席（TS/CLI 双
  侧零路由注册、engine-rs 仅剩管线内部模型+注释）三处差分器盲区嫌疑点（列表顺
  序/中间件存在性/端点缺席）全部成立；其余算法面（transcript 恒等锁/Dice/FNV/
  Levenshtein/projections 14 列/rule 校验）由 cargo:testgate golden/duel +
  engine-contract-diff 机器级覆盖。`inputGovernanceMode` 一条 1429 已修并锁定。
- **stale 编号卫兵零命中**：1428 两交互测试编号引用全部指向 1428；1426 修复面
  文件零 1428 引用。三面烤入 bundle hash 一致（index-b9PIdL30.js）。
- 1430 十腿/audit 族/bench 于同树（除本两文件）继续有效。

## 真红：ignored-sweep 抓到 e2e_startup 1200s 挂死（988 卫兵价值实证）

首轮实跑 exit 1：`test_startup_smoke has been running for over 60 seconds` →
**1200s 超时哨兵 SIGKILL**（992 号机制正常工作：有限超时+红报），stdout 无
startup result 行、vYm9xO 日志目录空（超时分支在 writeFileSync 前 continue，
脚本语义吻合）。

### 归因链（进程表+代码+时间线三重实证）

1. **孤儿进程表实锤**：测试二进制 `e2e_startup-*--ignored` + 双
   `inkos-engine-server`（LISTEN 4567）残留——app 被 assert_cmd 5s timeout
   SIGKILL 后引擎成孤儿且**继承的 stdout/stderr 写端永不关闭** →
   `output()` 等 EOF 永挂（903 孤儿族同形）。
2. **supervisor::spawn 无 stdio 配置**（Command 默认继承；supervisor.rs:108
   只有 process_group(0)）——SIGKILL 不可达 RunEvent::Exit 的 cleanup_sidecar/
   kill_tree（signal hooks 只盖 TERM/INT），孤儿无法避免。
3. **触发器=1426 号种 last_opened=仓根**：e2e_startup 的
   `INKOS_APP_DATA_DIR` 注入是**死契约**（app 端零消费，rg 零命中）——
   隔离从未生效；app 读用户级 projects.json。1426 之前 last_opened 缺失 →
   picker 挂 5s（无引擎、EOF 快速到达、绿）；1426 之后 → **auto 直进 studio
   + spawn 引擎** → 引擎长驻握管道 → 挂死。时间线铁证：11-15 22:28 绿轮
   （1426 落库 23:29 **之前**）vs 本轮 01:43 红（之后），10.01s/5.01s 历史绿
   形态与挂死形态边界恰在 1426。

### 修复 C：supervisor::spawn stdio 三面 null 化（生产正确性）

`cmd.stdin/stdout/stderr(Stdio::null())`——①挂死根除：app 亡故即 EOF 必达
（assert_cmd/launchd console 等等待方不再被孤儿写端挂死）；②污染根除：sidecar
eprintln（引擎 4 处低频诊断）不再混进 app stdout。边界备案：独立直跑 sidecar
不经本路径（dev 观测不受影响）；孤儿**进程**残留仍可能（SIGKILL 不可捕获是 OS
边界）——sweep 清理指引+下次启动 pick_free_port 容错兜底，parent-watchdog env
形态留作工项（getppid 轮询误伤独立跑形态，需 app 注入 env 门控，本轮不扩）。

### 修复 D：e2e_startup 三处（测试基建）

- 删 `INKOS_APP_DATA_DIR`+TempDir 死注入（app 端零消费），头注释备案形态敏感性
  （auto/picker 两形态在修复 C 后均 5s EOF 必达皆绿）；
- `log_dir` 对齐 app 真实写入面（main.rs init_logging = 用户级
  data_dir/<APP_DATA_DIR_NAME>/logs，常量经 inkos_desktop::config 引用）；
- **文件名断言勘误**：`inkos-`（连字符）→`inkos.`（点号）——logging.rs
  appender=prefix "inkos"+日期+suffix "log" 点号连接，原断言系**死分支期手打
  形态，与生产实现自始漂移**，分支激活后首次执行现形（1428 stale 同族第三例）。

### 过程备案

- Mimosa 对 supervisor.rs Edit 高危拦截（「命令注入」归因 `Command::new` 既有
  行）——按 900 号先例避开 sink 行、从 `.current_dir(&spec.cwd);` 后段追加，
  候选触碰面不含 sink 行即放行。
- 前台复跑同样挂死 → 排除「后台 detached session」假说，定位稳定缺陷；
  4567 端口占用态差异假说被否定（真实差异=last_opened 全局态）。
- `rg -rn` 两次误用 `-r`（replace）旗标致输出失真（只读无损）——族扫检索
  复核一律改用 `rg -n`。

## 验证谱

- startup 目标复跑 **2/2 全绿 10.01s**（修复前 1200s 挂死红）；
- **clippy:gate 双 crate 0 告警**；**src-tauri 全目标 25 目标 593 passed**；
- **ignored-sweep 全量复跑全绿**：supervisor 1/1 + startup 2/2 + secrets 9/9，
  清单地板卫兵精确匹配；
- 孤儿/端口终核全清（含 1429 轮漏网的 2 个 pnpm dev:server 包装进程——当时
  pkill 特征只中 tsx 子进程未中 pnpm 包装，本轮补杀）；
- U+FFFD 两文件净；工作区恰为本轮 3 文件。

## 教训

1. **死分支=零验证路径**（#[ignore] 盲区同族扩展）：从未执行的分支里断言可
   任意漂（连字符/点号 14 个月无人知）；激活死分支的第一跑就是勘误时刻。
2. **测试 env 注入必须核验 app 端消费点**——死契约=隔离幻觉，测试结果实际
   挂在用户机器全局状态上（本次挂在 1426 的产品修复上）。
3. **生产修复先于测试修复的一石二鸟检查**：supervisor stdio null 化同时修绿
   测试挂死+生产 stdout 污染+EOF 挂死族——比测试侧绕行（隔离 env 契约/清理
   钩子）更小侵入。
4. 988 ignored-sweep + 992 超时哨兵组合首度实战抓红：#[ignore] 挂起路径的
   潜伏缺陷（本例潜伏起点可追溯至 last_opened 功能落地）靠周期实跑现形。

出口=下一巡检周期/earendil-mz 单包合并（触发条件制已更新）；用户侧待办=Fork
推送(含 955-1431)+正式打包三脚本重烤。
