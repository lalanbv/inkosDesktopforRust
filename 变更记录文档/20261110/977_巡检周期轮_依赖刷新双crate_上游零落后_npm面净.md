# 977 号 · 巡检周期轮：依赖刷新双 crate + 上游零落后 + npm 面净

> 常备出口「下一巡检周期」本轮执行（903 号框架，距今 70 轮）。三线结论：
> **npm 面净**（pnpm outdated 0 行 + audit:npm 绿 + 848/854 四夹具必红自检活）、
> **上游协议零落后**（HEAD..upstream/master=0 新提交，本方领先 1499——+74 与
> 906-976 轮次吻合）、**cargo 面实质刷新**（engine-rs 16 项 + src-tauri 38 项，
> 双 lockfile 落地）。刷新后复验全绿：testgate 2110+584 持平零回归、audit:rust
> 双 0 自检活、clippy 双 0、gate:ts 十步全绿。
> 巡检轮无行为改动——必红形态=audit:npm 四夹具+audit:rust 双夹具自检活体确认
> （903 同例），无 NEUTER/stash 形。

## 一、三线巡检结论

### npm 面（净）

- `pnpm outdated`：0 行过时（workspace 全部最新）。
- `audit:npm`：✓ 绿；必红自检 4 夹具全活（未白名单告警→红 / 错误形状→红 /
  全白名单 id 钉定→绿 / 已白名单模块未钉定新 id→红）——848/854 形状卫兵
  机制活体确认。

### 上游协议面（零落后）

- `git fetch upstream` 后 `HEAD..upstream/master` = **0 新提交**；本方领先
  1499 提交（903 号时 1425，增量与 906-976 轮次吻合）。上游 2.0 无协议面
  变化，无需对照动作。

### cargo 面（实质刷新）

- engine-rs **16 项**：crc32fast 1.5.5、itoa 1.0.19、jiff×3（0.2.42）、
  sqlite-wasm-rs 0.6.3、syn 3.0.10、uuid 1.29.2、**zip 9.4.1→9.5.0**
  （846 事故重点观察面——golden 解包消费直测覆盖）等。
- src-tauri **38 项**：syn、tauri-plugin-notification 2.8.1、uuid、
  **wry 0.57.1→0.57.2**（连带 zbus 6.2.1 族/web_atoms/zruntime 新增——
  Linux webkit 依赖面展开，lockfile 445 行）。
- 双 lockfile 落地：engine-rs 138 行 / src-tauri 445 行。
- **备案**：`--verbose` 口径 engine-rs 11 包 / src-tauri 15 包 unchanged
  behind latest——semver 不兼容的新版本不进 compatible 刷新面（cargo
  update 默认语义），跨 major 面留待专项评估不盲推。

## 二、刷新后复验（全绿）

- `cargo:testgate`：engine-rs 52 目标 **2110 passed** + src-tauri 25 目标
  **584 passed**（与刷新前持平——依赖推进零回归；zip 9.5.0 下 golden
  解包面/EPUB 导出链直测覆盖无恙）。
- `audit:rust`：双 crate **0 漏洞**（engine-rs 2 条 / src-tauri 9 条
  unmaintained/yanked 警告维持）；必红自检双夹具（7+1 RUSTSEC）全活——
  847 机制活体确认。
- `clippy:gate` 双 0（依赖升级未带新 lint）。
- `gate:ts` 十步全绿（第 3 步 pnpm -r test 全量 + export-epub-smoke
  35.5s 回基线——zip 9.5.0 重编译二进制跑 EPUB 产物活体绿）。
- U+FFFD：lockfile 纯 ASCII 净。

## 三、巡检净结论与后续出口

- 三面巡检：npm 净、上游净、cargo 已刷新闭环。无漏洞无漂移，实质动作=
  依赖刷新（本轮完成）。
- 出口回归常备：上游协议扩展或外部事件或新真实需求或下一巡检周期；
  另 EPUB 字节级细节维持「如需」备案（结构级 golden 已覆盖，字节级全等
  因运行时值不成立——974 定性）。

**下一号自 978 起；用户侧待办=Fork 推送（含 955-977）。**
