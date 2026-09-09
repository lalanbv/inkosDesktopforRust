# 259 号：自主持续开发第 2 轮——bench:gate 负载前置检查 + 引擎 panic 面审计（一真修）

- 日期：2026-09-10
- 分支：develop
- 关联：257 号（遗留 #2 清偿 + 噪声协议落地）、project_config_routes 非对象根韧性先例
- 编号说明：本篇原编 258 号，与并行会话 8972a169（256 号评审修复）撞号，按「开工先查并行会话」规范让位改 259 号
- 推送核验：HEAD 8972a169（并行会话 258 号已提交）；本地领先 origin/develop 59+；本批（含 257 号清理批）待提交

## 一、批A：bench-gate 负载前置检查（257 号遗留 #2 清偿）

`scripts/bench-gate.mjs` 增加执行前负载门：

- **判据**：1 分钟 loadavg / 核数 > `maxLoad`（默认 0.20，实测 0.23 即出现 +190%~+520% 假漂移）→ 拒绝执行，exit 3（区别于 bench 失败 1 / 参数错 2）。
- **可调**：`--max-load 0.3` 或 `BENCH_GATE_MAX_LOAD`；`--ignore-load` 强跑自担。
- Windows 无 loadavg（恒 [0,0,0]）自动通过。
- **验证**：拒绝路径（--max-load 0.1 正确拒 + 报文清晰）、非法阈值路径（exit 2）、放行路径各一遍；且落地后**整套门禁真跑全绿**（9 基准全部收敛在 ±3%，exit 0）——257 号两轮"大幅回退"确证为负载噪声，基线无需刷新。

## 二、批B：引擎生产路径 panic 面审计

**范围**：server/* + bin/* 非测试代码全部 unwrap/expect/panic 位点逐一核 origin。

**结论分级**：
- 安全惯用法（不动）：`Mutex::lock().unwrap()`（毒化级联，作用域极小）；`OnceLock` 静态正则 `Regex::new(..).unwrap()`（编译期可证）；`json!` 自构造后的 `as_object_mut().unwrap()`（构造即对象，约 10 处）；前置校验后的 `expect("已校验")`。
- **[已修] 唯一真实缺口**：`books_state_routes.rs` `load_raw_book_config` 不做对象守卫——book.json 为合法 JSON 但非对象根（手编成 `[1,2,3]`/`42`）时，`put_review_mode`/`put_timeline_auto_beats` 等四调用方中 `raw_book.as_object_mut().unwrap()` **panic 打断连接任务**（客户端见连接重置而非错误响应）。TS 同路径不崩（数组取 `.writing` 为 undefined → 回退默认；PUT 静默无损）。

**修复**：`load_raw_book_config` 解析后非对象根返回 `InvalidData` io 错误 → 四调用方既有 `Ok(..) else → 404` 分支承接，与 GET 路由注释的文档语义（"book.json 不可解析 → 404"）一致，亦对齐 project_config_routes 的"非对象根韧性"先例语义（不 panic、干净错误码）。新增韧性回归测试 `non_object_book_json_returns_404_not_panic`（覆盖 GET/PUT × review-mode/auto-beats 四路径）。

## 三、验证

| 门禁 | 结果 |
|---|---|
| engine cargo test（INKOS_DUEL=1 全量） | ✓ **1618 通过 0 失败**（含新增韧性测试，较 257 号 +1） |
| engine clippy --all-targets | ✓ 0 |
| bench:gate | ✓ 真跑全绿（±3% 内），负载门生效 |

Node 侧与 src-tauri 本批未触及（上轮门禁仍有效）。

## 四、遗留

1. 历史瘦身、推送、两项默认值、ja A/B——仍待用户决策（承接 257 号）。
2. panic 面审计可扩展至 `src/pipeline`/`src/interaction` 写路径深水区（本轮 server/bin 面已清，其余抽查未发现外部可达 panic）。
