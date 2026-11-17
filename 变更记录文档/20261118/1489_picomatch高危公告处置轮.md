# 1489 号：picomatch ReDoS 高危公告处置轮——gate:ts 补证活体抓出+白名单 id 钉定

## 目标与范围

/goal 自主持续开发续轮。先做系统性债信号扫描（无集中失控债），
后识别真实验证缺口：**1486 依赖刷新（50 包）+1487 fmt（402 文件）
后 gate:ts 十腿未整跑**——本轮整跑补证，**活体抓出新安全事件**并
处置。

## 债信号扫描（定性：无集中失控债）

- TODO/FIXME/XXX 双栈源码**零残量**；
- `#[allow]` 80 处全有注释理由（48 too_many_arguments 真实签名
  形态+14 dead_code 刻意占位如 serde 捕获字段/诊断预留+14
  too_many_lines 等）——有纪律的压制面非失控债；
- unwrap() 3414 处=980 号已定性面（锁 unwrap 惯用形为主，三面
  实证零高危）。

## gate:ts 整跑——活体抓出+处置

**首轮九绿一红**：build/typecheck/test/rust-bin（fmt 后二进制
同源校验过）/node-fallback-smoke/engine-contract-diff（**1486+1487
后双栈对照绿**）/export-epub-smoke/work-alternation/check-routes
九腿绿；**audit:npm 红**——新告警：

- **picomatch GHSA-fc5q-39xc-vjpf**（CVE-2026-112366，high，
  单分支重复 extglob 致 ReDoS）——1486 巡检后新发布的 advisory
  （当时 4 白名单全绿）。

**定性三面**：
1. 依赖树位置：vite→fdir/tinyglobby→picomatch 与
   shadcn→dotenvx→fdir 等链，**全部 devDependencies 构建工具链**
   （pnpm why 实证，生产依赖零命中）；
2. 补丁线：**patched: None**（≤4.0.7 全线受影响，4.0.7 即最新
   终态——758 deepmerge「无补丁线终态」同型先例），升级路径不存在；
3. 暴露面：ReDoS 须攻击者可控 glob 模式入参——构建工具的 glob
   输入为本仓自家源码文件名/配置，无不可信输入面；运行时（桌面
   应用/引擎）零执行。

**处置**：audit-npm.mjs 白名单加 picomatch 条目（854 纪律=id 钉定
形态：仅放行 GHSA-fc5q-39xc-vjpf 一个 id，同模块未来新 advisory
仍拦截——第 4 必红夹具锁该路径）；移除条件=上游发布补丁线
（4.0.8+）随 vite/fdir/tinyglobby bump 出清。

## 验收

- audit:npm 复绿：advisory 合计 5 白名单放行 5+四夹具自检全活；
- **gate:ts 十腿整跑全绿 exit 0**（含 fmt/依赖刷新后双栈对照
  engine-contract-diff 与 rust-bin 同源校验）——1486+1487 的
  TS 侧验证缺口闭环。

零新缺陷（新公告定性处置=正收益），U+FFFD 净。

出口=下一巡检周期（约 30 天后，2026-12 中旬前后到期）/earendil-mz
单包合并；用户侧待办=**Fork 推送（含 955-1489，Fork 图形端操作）+
正式打包三脚本重烤（必须含 engine-rs release 重编——1486 依赖
刷新+1487 fmt 后重编必要性加码）**。下一号自 1490 起。
