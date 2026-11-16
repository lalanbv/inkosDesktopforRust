# 1450 号：ignored-sweep 当前树首跑 + 全量双窗五证轮（零代码改动）

## 目标与范围

/goal 升格「全量」→ 1433 清单制。非冗余增量判定：十腿+audit 族=1447 同树
谱系（166c801f..HEAD 纯 docs）；bench=1448 定性制（会话内不重试）；本
会话唯一未跑的电池腿=**cargo:ignored-sweep**（挂起路径地板卫兵——M1a
两枚+988 一枚潜伏缺陷历史全在挂起路径上，1436/1437 动过脚本自身、
1444 卫兵改动后从未实跑）→ 本轮实跑补齐。

## ignored-sweep 当前树首跑全绿（本轮核心增量）

`pnpm cargo:ignored-sweep` exit 0：

- supervisor_integration **1/1** + e2e_startup(--release) **2/2** +
  e2e_secrets **9/9** = **12 枚挂起用例清单精确匹配全绿**（绿档日志已
  清盘——1436/1437 清盘机制活体）；
- 1444 卫兵改动树（books_routes 8 卫兵+锁测）后挂起路径零潜伏回归；
- **[post-clean] 引擎孤儿回收 1 枚**（PPID=1+本仓路径特征）——1435
  自愈闭环在生产中工作。

## gate:ts 十腿全绿（971c9a54 全量首谱，将错就错升格）

预置「便宜腿微谱」时 `gate-ts.mjs --only` 旗标不被支持→全量起跑，顺延
为**当前 HEAD 首谱**：build 20.7s / typecheck 20.6s / **test 102.8s** /
audit:npm 4.8s / rust-bin 0.3s（活体二进制 cargo 指纹核验新鲜）/
node-fallback-smoke 35.4s / engine-contract-diff 36.3s / export-epub-smoke
35.0s / work-alternation 5.1s / check-routes 0.1s——exit 0。（附带勘误：
`pnpm check-routes` 非独立脚本，该腿由 gate:ts 内嵌覆盖——独立调用报
Command not found 为命令误用非门禁红。）

## 泄漏普查（第五证：sweep+gate 双窗口）

ignored-sweep（cargo 面）+gate:ts 十腿（TS 面+三 smoke）**双窗口后
TMPDIR inkos-* 恒 5 设计内**（产品运行时+固定名 mock log×3+
m1-demo-realtest）——1440/1442 修复第五次独立实证，且首次含 cargo 测试
面窗口（此前四证均 vitest/smoke 窗口）。

## 引证链与巡检触发面

- audit 族（audit:rust 双夹具活/semantic 候选 21∩改动面=0/bindings
  186/manifests 三包/clippy 双 0）+ 密封 seal f44ba29e：1447 同代码树
  引证在位；
- Rust 面：0bd1ca83..HEAD 纯 docs（3 文件 193 行）→ 1444 testgate
  （2115/593）谱系在位；本轮 ignored-sweep 12 枚绿为其挂起路径补充；
- bench：1448 定性制（会话内不重试，用户侧静默补跑可选）；
- 巡检：fetch 后 behind=0 + earendil* 0 分支 + 快照/watchdog 周期未到
  （1435 当日已完成）。

## 结论

清单对照：十腿 ✓（971c9a54 首谱）/ ignored-sweep ✓（当前树首跑 12 枚
全绿）/ audit 族引证 ✓ / bench 定性制 ✓ / 泄漏第五证 ✓（双窗首含 cargo
面）/ 巡检 ✓。零新问题、零修复需求、零代码改动。U+FFFD 净。

出口=下一巡检周期/earendil-mz 单包合并；用户侧待办=Fork 推送(含
955-1450)+正式打包三脚本重烤+bench 静默补跑（可选）。下一号自 1451 起。
