# 674 号：Rust 腿三分支活体+tracing-subscriber bench 回退实验（引入→确证→回退定案）

日期：2026-10-06。类型：docs(Rust 腿分支活体+性能实验归档)。前置：673 备案「未产出/未净提升/回退分支未逐一注入」；673 备案「Rust 阶段日志不打 stdout」的根因勘察。

## 一、Rust 腿三分支活体验证（674a/b/c/d 四轮 env）

| 分支 | 注入 | Rust 管线行为 | 落盘 |
| --- | --- | --- | --- |
| 未产出 | 45,91+PASSTHROUGH | 修稿直通回传原文→reviser 判与 finalContent 全等→未产出退出 | 初稿 2405/无替换句/audit-failed |
| 未净提升 | 45,47,91+retries=2 | 修 PATCH1→复审 47<45+3→未净提升退出 | 初稿 2405（修订被丢弃）/audit-failed |
| restore 回退 | 45,91+BLOAT | 修超长版 91 分越界→bestSnapshot 择优初稿→回退 | 初稿 2405/无超长尾段/audit-failed |

三分支在 Rust 腿逐一活体（叠加 673 patch-only 主路径+达标退出）——**Rust 实现审改循环六分支全部有活体证据**。

## 二、tracing-subscriber bench 回退实验（引入→确证→回退定案）

- **根因发现**：engine-rs bin 从未初始化 tracing subscriber——管线 tracing 日志（审改循环 logStage/logWarn）全盲（tracing 宏默认 no-op）。首版修复=Cargo.toml 加 tracing-subscriber + bin fmt().init()——Rust 管线日志全可见 ✓。
- **bench 红**：dispatch/1 三连红 **156.61(+44.9%)→228.42(+111.4%)→369.76(+242.2%)** ns（基线 108.06）；dispatch/8、/32 仅 +3~7% 带内。
- **鉴别**：改为「RUST_LOG 显式设置才 init」的条件版（运行时不执行 init）——**仍红 228ns** → 劣化非 init 执行、而是**依赖引入本身**（链接布局/静态初始化对纳秒级 hot loop 的副作用）。
- **回退实验**：git checkout 基线代码重 build → bench 通过（**dispatch/1 +9.6%=118ns 带内**）——因果确证。
- **定案**：**回退全部 Rust 改动**（依赖+bin 条件块）——Rust 管线日志维持盲态，改为备案（673/674 的分支验证以 API 状态+落盘痕迹为证据，不依赖日志）。

## 三、bench 三红三绿实验数据

红：+44.9% / +111.4% / +242.2%（tracing-subscriber 在）；绿：+9.6% / +9.5%（基线代码）——同一代码版本两次绿（+9.6%/+9.5% 稳定）证明测量系统本身稳定，红绿差异确系依赖。

## 四、终态

- mock/套件改动保留（PASSTHROUGH_FROM+LAST_BODY 轮转+场景 G2+PATCH3 对——scripts 面无 Rust 影响），七场景+G2 全绿。
- Rust 腿六分支矩阵全活体（673 patch-only+达标、674a 未产出、674b 未净提升、674d restore、655 多轮上限 2/2、**667 三轮净提升链 3/3**——retries=3 顶格轮数首次活体）。
- Rust 管线日志盲态备案定案：Rust bin 日志面改造（不引 tracing-subscriber 的轻量方案[eprintln 手写管道]或接受盲态+API/落盘面验证）为独立长尾，触发=真实排查需求。

## 五、教训

1. **tracing-subscriber 的全局注册对纳秒级热路径有实测 +45~242% 副作用**（即使 init 在运行时不执行——依赖链接与二进制布局的静态效应）——性能敏感 bin 引入全局观察者依赖前必须跑 bench 验证零回退；本项目 Rust 管线日志采用「默认盲+按需方案另议」。
2. **鉴别的决定性手段=回退实验**：条件 init 仍红排除了「运行时开销」假设；checkout 基线复绿确证「依赖引入」因果——两次实验闭环避免了对机器状态/噪声的误归因。
3. bench 红线的排查顺序沉淀：隔离复跑排除噪声 → 基线 checkout 排除代码 → 逐依赖二分定位——本次三步全走完。
4. Rust 管线日志全盲的排查代价（673/674 依赖 API/落盘面间接证据）=该长尾的真实成本记录；轻量方案（eprintln 手写管道重定向 tracing 输出或管线 log 函数化）留给真实排查需求触发。
