# 903 号 · 下一巡检周期轮：依赖刷新双 crate + 上游零落后 + audit 双栈活体确认

> 常备出口「下一巡检周期」本轮执行。三线巡检结论：**npm 面净**
> （pnpm outdated 0 行=全部最新 + audit:npm 绿+848 四夹具必红自检活）、
> **上游协议零落后**（HEAD..upstream/master=0 新提交，本方领先 1425，
> 与 722 号实证一致）、**cargo 面真实刷新**（engine-rs 11 包+
> src-tauri 17 包 compatible 更新，双 lockfile 落地）。刷新后复验：
> testgate 2044+584 全绿、audit:rust 0 漏洞维持且 engine-rs 警告
> 9→2（zip 更新消 unmaintained 警告）、clippy 双 0、gate:ts 八步
> 全绿（结果回填于提交信息）。

## 一、三线巡检结论

### npm 面（净）

- `pnpm outdated`：0 行过时（workspace 全部最新）。
- `audit:npm`：✓ 全部为已接受风险（dev 工具链）；必红自检 4 夹具
  （未白名单告警→红 / 错误形状→红 / 全白名单 id 钉定→绿 / 白名单
  模块未钉定新 id→红）全活——848 形状卫兵机制活体确认。

### 上游协议面（零落后）

- `git fetch upstream` 后 `HEAD..upstream/master` = **0 新提交**；
  本方领先 1425 提交。上游 2.0 无协议面变化，无需对照动作。

### cargo 面（真实刷新面→修洞按族执行）

- engine-rs：11 包 compatible 更新（aes 0.9.4、cc 1.11.1、flate2、
  jiff×3、mime、serde_json 1.0.154、smallvec、uuid、**zip 9.2.2→9.4.1**）
  ——zip 有 9.1 critical 挂数周前科（846 号先例），版本推进属实质
  安全卫生。
- src-tauri：17 包 compatible 更新（async-lock、brotli-decompressor、
  cc、flate2、jiff×3、mime、serde_spanned、smallvec、**toml
  1.1.8+spec→1.2.1+spec** 族、winnow、uuid、zip 9.4.1 等）。
- 双 lockfile 落地：engine-rs 46 行 / src-tauri 132 行（89+89 对称
  增删，纯版本推进无结构变化）。

## 二、刷新后复验（全绿）

- `cargo:testgate`：engine-rs 51 目标 **2044 passed** + src-tauri
  25 目标 **584 passed**（与刷新前持平，依赖推进零回归；zip 9.4.1
  重点观察面无恙）。
- `audit:rust`：双 crate **0 漏洞维持**；engine-rs 警告 **9→2**
  （zip 9.2.2→9.4.1 消解 unmaintained/yanked 警告——刷新的实质安全
  收益）；必红自检双夹具（7+1 RUSTSEC）全活——847 机制活体确认。
  yanked 联网核验超时 328 次属网络抖动（非漏洞，备案）。
- `clippy:gate` 双 0（依赖升级未带新 lint）。
- `gate:ts` 八步全绿（数字回填提交信息）。

## 三、巡检副产物：smoke 孤儿池发现与清场修复（按 846 修洞族）

- **发现**：gate:ts 首次满载跑（lockfile 重编译语境）各步 10~40 倍漂移
  （rust-bin 641.9s、export-epub-smoke 1573.0s），复跑 test 步 1639.6s
  假失败+typecheck 2008.1s——根因非依赖回归，是 **~596 个 detached 孤儿
  进程池**（tsx preflight 298+studio api 298，带 inkos-smoke-node-*
  tmp root 特征）积累拖垮系统；路径限定 pkill 清理后干净复跑八步全绿
  全回基线（test 102.5→99.4s、smoke 35.9/35.6s）=依赖刷新零回归定案。
- **泄漏面**：两 smoke 脚本 `spawn(detached:true)` 的 api/mock 进程组
  不随父死——脚本异常死亡时无人收尸。
- **信号兜底实证不可靠（谜团备案）**：smoke 主进程注册 SIGTERM handler
  后对 TERM **完全免疫**（handler 0 触发、进程不终止；sample 栈=
  SpinEventLoop 空转），同法最小复现正常——Node 26 detached 语境信号
  语义未钉死，不硬啃。
- **落地（不依赖信号语义的确定面）**：①双 smoke 脚本 spawn 前清场
  （pkill 特征限定本家族 tmp root `inkos-smoke-node-/rust-/epub-smoke-`
  与全仓唯一的 walkthrough-mock，不误伤 studio dev/用户进程）——上一
  轮孤儿下一轮回收，防积累；②exit 钩子保留（正常/异常默认退出路径收
  尸）；③无效信号 handler 撤除（0 触发实证，假防线违背正向确认纪律）。
- **验证**：造孤儿（特征命令行）→跑 smoke→孤儿被清+功能全绿+实况零
  残留；语法 node --check 净；gate:ts 八步全绿（含两 smoke 改动面）。

## 四、巡检净结论与后续出口

- 三面巡检：npm 净、上游净、cargo 面已刷新闭环+孤儿池清场修复。
  无漏洞无漂移，实质动作=依赖刷新+孤儿清场（本轮完成）。
- threads 扩散面饱和（902）；备案链 898→903 全清偿；本轮后出口
  回归常备：上游协议扩展或外部事件（pi1.0 key）或新真实需求或
  下一巡检周期。

**下一号自 904 起；出口清空回归常备；用户侧待办=Fork 推送 37 个提交。**
