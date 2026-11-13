# 1016 号 · node_min 抬升专项轮——bootstrap 22.19.0 + 防漂移卫兵 + 校验链活体

> 1015 出口两候选权衡：node_min 先行（**解锁型小专项**——typebox 统一 35 文件
> 大专项的前置路径打通）先落，typebox 专项顺延 1017。1014 勘察的 node_min
> 冲突族三载体（vite 22.12/pi 1.5.2 22.19/unpdf 2 24）的共同前置在此收口
> （unpdf 的 24 留触发条件，22.x 线内先满足 vite+pi）。

## 一、普查发现：node_min 是纸面元数据，真缺口在 bootstrap 实体

- **声明源 2 处**：`scripts/desktop-package-engine.sh:78 NODE_MIN="22.0.0"`（打包时写入 manifest.json）+ `src-tauri/src/engine/manifest.rs:83`（测试 sample）。
- **运行时零消费方**：node_min 在 manifest.rs 之外无人读取——纯诊断/审计元数据，无运行时校验。
- **真缺口**：`engine/node/mod.rs NODE_BOOTSTRAP_VERSION = "22.11.0"`——Node 回退路径 bundled 的 Node **实体** 22.11.0 < pi 1.5.2 engines 下界 22.19。即使抬 manifest 声明，bundled 实体不满足=纸面 node_min 是谎言。
- bootstrap 逻辑=纯常量 resolve 到缓存（`cache_key(ver)` 随版本自动换新），无系统 node 比较；已装 22.11 缓存的用户升级后首启自然重新 bootstrap（预期行为）。

## 二、落改（三声明 + 双卫兵）

1. `NODE_BOOTSTRAP_VERSION` 22.11.0 → **22.19.0**（注释锚定 1014 勘察与冲突族背景）。
2. `manifest.rs` sample node_min → "22.19.0"（测试样例与生产同值）。
3. `desktop-package-engine.sh` NODE_MIN → "22.19.0"。
4. **防漂移卫兵双测试**（node/tests.rs）：
   - `bootstrap_version_matches_packager_node_min_declaration`：include_str 读打包脚本，断言 NODE_MIN 与 NODE_BOOTSTRAP_VERSION 同值——845 号同族双声明教训（两处声明必漂移）机制化；
   - `bootstrap_version_satisfies_pi_1_5_engines_floor`：semver 元组断言 ≥(22,19,0)——pi 再抬 engines 时测试红=提醒复核。

## 三、红绿与活体

- 绿态：engine::node 12/12（含双卫兵）。
- **NEUTER1016**：版本暂回 22.11.0 → **恰红 2/2**（engines 下界断言+同值漂移断言各自触发，报错信息按设计输出）→ 互逆恢复。
- 全量：src-tauri 25 目标 0 FAILED + clippy 零告警。
- **校验链活体**：nodejs.org v22.19.0 dist 可得（HTTP 200）+ 真实下载 47.7MB + **官方 SHASUMS256 比对通过**——bootstrap 的下载→校验链对新版本实体成立。

## 四、备案

- **unpdf 2 的 Node 24+**：不在本轮（22.x LTS 线内先满足 vite+pi）；node_min 24 抬升与 unpdf 2 升级绑定为同一未来触发。
- **升级用户行为面**：bundled Node 缓存 key 随版本变化（`{ver}-{os}-{arch}`），升级后首启重新下载 22.19.0（~48MB）——镜像/网络受限环境的用户回退系统 node 路径不受影响。
- **运行时校验缺失备案**：node_min 目前无运行时消费方（spawn 前不校验系统 node 版本）——若未来 pi 引擎在低版本 node 上产生难懂报错，可考虑 spawn 前按 manifest.node_min 校验并给可行动错误（1014 定性延续，非本轮）。

## 五、下一号自 1017 起

出口=**typebox 0.34→1.x 统一专项**（35 文件，mz 0.73.1 升级前置，node_min 前置已解锁）/ 用户侧待办=Fork 推送（含 955-1016）。
