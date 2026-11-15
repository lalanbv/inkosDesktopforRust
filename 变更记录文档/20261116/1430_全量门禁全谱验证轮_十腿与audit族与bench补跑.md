# 1430 号：全量门禁全谱验证轮——十腿 + audit 族 + bench 补跑到点（零代码改动）

## 目标与范围

/goal 全量测试新增代码逻辑。1425 号之后历经 1426/1427/1428/1429 四轮改动，最近一次
完整十腿还停在 1425 时点；本轮=全谱门禁实跑+1429 修复面独立终审+1425 遗留 bench
补跑待办清偿。零代码改动（纯验证轮），树=HEAD 715c73c4 未动。

## 全谱验证结果

①**gate:ts 完整十腿全绿**（1429 修复树首谱）：build 19.4s / typecheck 19.5s /
test 107.2s / audit:npm 3.0s / rust-bin 1.3s（活体二进制 cargo 指纹新鲜）/
node-fallback-smoke 35.9s / engine-contract-diff 36.1s 双栈零分歧 /
export-epub-smoke 35.0s / work-alternation 5.1s / check-routes 0.1s。
关键面备案：node-fallback-smoke=修复 B（schema 容键）核心受益面（Node 回退腿启动
路径）活体健康实证；engine-contract-diff=修复 B 触碰双栈契约面（ProjectConfig）
后 Rust/TS 零新分歧。

②**audit:rust 日更复查绿**：双 crate 0 漏洞（9 条 unmaintained/yanked 警告；yanked
联网核验超时 336 次=网络抖动非漏洞）+ 必红自检双夹具活体（known-vulnerable 检出
7 RUSTSEC / single 检出 1 RUSTSEC——检测器非恒绿，847 号机制在位）。

③**audit:semantic-patterns 稳态终审**：candidates=41 恰等于 1425 时点稳态（850
清零后备案值）；本轮四改动文件（use-api/models/project/config-writing-keys/App）
零新候选；exit 1 为 findings>0 工具本义（850 已裁决接受稳态）非门禁红。

④**verify:publish-manifests** 三包 OK；**verify:engine-bindings** 205 passed /
186 bindings 全绿。

⑤**bench:gate 补跑到点**（1425 号遗留待办清偿）：卫兵 1min loadavg 3.41/18 核
=18.9%<20% 阈值放行（15 行注释备案的判定窗口），二十轮热路径全谱实跑门禁通过，
各基准漂移处于噪声带/改善向（tool_registry/lookup_miss +10.1% 等均远低于 1.3
阈值；本轮 1426-1429 零 Rust 改动，归因环境噪声，按 bench-gate-load-noise 纪律
不逐个隔离复跑、严禁 --update 洗基线）。

## 1429 修复面独立终审（本轮新做）

- 修复 A 正则：alternation+可选组线性结构无嵌套量词=无回溯爆炸面；模块级字面量
  编译一次、`.test()` 零分配、仅启动错误冷路径调用=0GC 面；带尾空格串不命中=
  fail-safe 方向（错指 inkos.json 而非误判不可达，保守正确）。
- 修复 B schema：`z.unknown().optional()` 对 ProjectConfig 类型面=纯拓宽（只增
  可选字段），typecheck 双绿佐证；输入来自 JSON.parse 无函数面，zod object 显式
  字段访问无原型合并新风险；写回面直读 raw JSON 零副作用（1429 已定性，本轮复核）。

## 模拟真人测试面

1429 号当轮已做活体三腿第一手（裸 500 分流/ECONNREFUSED 回归/Retry 自愈→仪表盘），
本轮零代码改动树未变（HEAD 同 715c73c4），上轮活体证据对当前树继续有效，按
「避免重复已完成工作」纪律不重跑。

## 清单对照（1425 全谱先例）

gate:ts 十腿 ✓ / audit:rust ✓ / semantic-patterns ✓ / bench:gate ✓（1425 拒跑
本轮补齐）/ verify:publish-manifests ✓ / verify:engine-bindings ✓——全谱覆盖，
零回归、零新问题、零修复需求。

出口=下一巡检周期/earendil-mz 单包合并（触发条件制已更新）；用户侧待办=Fork 推送
(含 955-1430)+正式打包三脚本重烤。
