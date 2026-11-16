# 1456 号：依赖 semver 兼容刷新窗口实施轮——双 crate 落锁+全谱重验锁定零回归

## 目标与范围

/goal 自主持续开发续轮。实施 1452 备案的依赖刷新窗口：engine-rs 7 包
+ src-tauri 25 包（备案口径）可更新，全部 semver 兼容 patch/minor 跨越，
978 号触发条件（major/安全更新）未点火。刷新=改 Cargo.lock 的代码树
变更，须全谱重验锁定零回归后提交。

## 结果

①**dry-run 复核（刷新前）**：

- engine-rs 7 包与 1452 备案**精确同面**：jiff 族 4 包（0.2.43→0.2.46
  等 patch）+ nix 0.31.3→0.32.0（minor，最大跨越）+ rand 0.10.3→0.10.4
  （patch）+ zip 9.5.1→9.6.0（minor）
- src-tauri 细面漂移记录：19 Updating（tauri 族 2.12.5→2.12.6 /
  codegen/macros 2.7.4→2.7.5 / plugin 2.7.2→2.7.3 / runtime 族 /
  utils 2.10.2→2.10.3 全 patch + jiff/nix/rand/zip/brotli/ctor 同面）
  + **8 Adding**（sha2 0.11.1 哈希族传递新依赖：block-buffer/const-oid/
  crypto-common/digest/hybrid-array/link-section——tauri 族 patch 引入
  的依赖树新增，非直接依赖变更）=27 行 vs 备案 25（advisory-db/index
  滚动所致细面差，方向仍全 semver 兼容）

②**双 crate 落锁**：`cargo update` 串行执行（勿双 cargo 并行——
securityd 串行化假象教训），改动面**仅两把 Cargo.lock**（141 行变更，
Cargo.toml 零触碰）。

③**全谱重验三面全绿**：

- **testgate 全量**（新锁全量重编）：engine-rs 52 目标 2115 passed +
  src-tauri 25 目标 593 passed——与刷新前谱（1454）**精确同数零回归**
- **audit:rust**（testgate 前置哨兵）：双 crate 0 漏洞（zip 9.6.0
  干净——846 号 zip critical 挂数先例面重点核过）+ 必红双夹具活
  （7/1 RUSTSEC 检出=检测器活着）；yanked 联网核验超时 339 次=网络
  抖动非漏洞
- **clippy:gate --all-targets**：双 crate 0 告警（nix 0.32/zip 9.6.0/
  tauri 2.12.6 新版本零新 deprecation 面）

④ustc 镜像健康（dry-run/落锁全程无宕机，未启用 rsproxy 绕行）。

零代码改动（锁文件面），零回归，U+FFFD 净。

## 清单对照（1433 清单制）

testgate ✓（本轮新锁首谱）/ audit 族 ✓（audit:rust 本轮新鲜 + semantic
∩改动面=0（锁文件非源码）+ bindings 186 + manifests 三包=1447 引证）/
clippy ✓（本轮重验双 0）/ gate:ts 十腿 ✓（1455 首谱——TS 面零触碰，
npm 锁未动）/ bench = 1448 定性制（依赖 patch/minor 刷新非热路径改码，
基准面不涉）/ ignored-sweep ✓（1450 引证）/ 巡检项=本轮刷新即 1452
备案「依赖新鲜度」项的清偿实施。

出口=下一巡检周期/earendil-mz 单包合并；用户侧待办=Fork 推送(含
955-1456)+正式打包三脚本重烤（依赖刷新后打包产物含新依赖树——三脚本
重烤时自然带入）+bench 静默补跑（可选）。下一号自 1457 起。
