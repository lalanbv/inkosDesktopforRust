# 177 · W-D4 性能门禁：热路径 criterion 基准 + bench-gate 门禁 + 敏感词扫描 13.5x 优化

- 日期：2026-09-07
- 模块：engine-rs（benches/hot_paths.rs 新增、Cargo.toml、src/agents/sensitive_words.rs）、scripts/（bench-gate.mjs + bench-baseline.json 新增）、package.json、总体方案文档
- 类型：feat + perf + chore
- 关联：总体方案 W-D4（原定对象重选型）、[重构优化总体方案_v1](../../开发时SpecCoding'sPlan/inkosDesktop/06_全局重构优化/重构优化总体方案_v1.md)（「profiler/证据先于假设」纪律）

## 一、背景

W-D4 backlog 原定 bench 对象（plugin_execute/scanner）源自上游分析文档，在本仓不存在——本批按**实际调用频度**重选两条热路径：

1. **`analyze_sensitive_words`**：审计/全周期审计/合并审计每章必经的纯函数扫描（内置政治/色情/暴力三组词表 + studio 自定义词表）。
2. **`BroadcastHub::broadcast`**：每个写面事件（write:start/progress/complete…）的同步分发跳。

按纪律先建基准拿证据，基线一跑即抓到真问题：

> `scan_words_impl` 对**每个词每次调用**执行 `regex::escape` + `Regex::new` 重编译——注释自称「词表固定，非热路径」，但它恰恰坐在每章审计热路径上。基线：`scan_chapter`（内置+50 自定义词,~9KB 中文）**224.5µs/章**,其中逐词编译占绝对大头（自定义 50 词独占 ~120µs,内置三组 ~105µs）。

## 二、改动

### 1. criterion 基准（`engine-rs/benches/hot_paths.rs`，harness=false）

- `sensitive_words/scan_chapter`：~3000 中文字章节 + 50 自定义词（2 命中）,吞吐维度带 bytes。
- `sensitive_words/scan_chapter_builtin_only`：仅内置三组的最小形态。
- `sse_broadcast/dispatch/{1,8,32}`：BroadcastHub 同步 broadcast（订阅者后台 drain,订阅者数为真实变量）。
- Cargo.toml：`[[bench]]` + dev-dep `criterion 0.5`。

### 2. 敏感词扫描优化（语义零漂移）

取证发现 TS 原实现（`packages/core/src/agents/sensitive-words.ts` L127-135）是 `new RegExp(escapeRegExp(word), "g")` + `content.match(regex)`——**escape 产物是字面正则,只匹配字面子串**,与 Rust `content.matches(word).count()`（memmem,零编译）完全等价（空词两侧同为「每位置一次空匹配」）。实施中途曾走 OnceLock 编译缓存（内置表 -95.8%）,复核等价性后改为**纯子串扫描**——更快、更简单、缓存机制整体退役、自定义词表同步受益：

| bench | 基线（逐词 Regex 重编译） | 子串扫描 | 提升 |
| --- | --- | --- | --- |
| `scan_chapter`（内置+50 自定义） | 224.5 µs | **16.6 µs** | **13.5x** |
| `scan_chapter_builtin_only` | 105.5 µs | 9.6 µs | 11.0x |
| `sse_broadcast/dispatch/*` | — | ~128-131 ns | 基线首采 |

语义锁定：`sensitive_words` 8 个既有单测全绿（含政治 block/双语/自定义词/空文本）+ engine-rs 全量 1232 lib。

### 3. bench-gate 门禁（`scripts/bench-gate.mjs`，`pnpm bench:gate`）

- 跑 `cargo bench --bench hot_paths` → 解析 `target/criterion/*/new/estimates.json` 均值 → 与入库基线 `scripts/bench-baseline.json` 逐项对比 → 超阈值（默认 +30%,`--threshold` 可调）非零退出并列明细。
- 首跑无基线自动采集写入;`--update` 显式刷新;`--list` 只看基线;`--` 之后透传 criterion 参数（`-- --quick` 快速自检,正式门禁全量跑——quick 采样在 dispatch/1 实测可抖 ±100%,不入基线）。
- 新增/消失 bench 提示登记/清理（不 FAIL）。基线记录采集时间与 host（机器相关,换机重建）。

## 三、验证

| 项 | 结果 |
| --- | --- |
| `engine-rs cargo test` 全量 | 全绿：1232 lib + 194 + 70 + 8,0 失败 |
| `engine-rs cargo clippy --all-targets -- -D warnings` | 零告警（含 benches 编译） |
| 门禁通过路径 | `bench:gate -- --quick` 对全量基线：5 项波动 -0.3%~+5.5%,exit 0 ✓ |
| 门禁告警路径 | 篡改基线（scan_chapter→1µs）→ exit 1 + 明细「1.00 µs → 16.46 µs（+1546.2%）」✓;恢复后全量 `--update` 重采入库 |
| 优化对照 | 见上表;criterion 全量均值 `scan_chapter` 16.6µs（95% CI ±0.05%） |

## 四、过程教训

1. **注释也会说谎**：「词表固定，非热路径」的注释与真实调用频度（每章审计必经）相反——热路径判定以调用图为准,不以注释为准。
2. **等价性复核能简化方案**：中途的 OnceLock 缓存方案（-96%）在复核「escape 正则 = 字面子串」后被更简单的纯 `str::matches` 替代（-96% 且删掉整个缓存机制,自定义词表同步受益）——优化前先问「这个抽象在做什么,有没有更原始的等价原语」。
3. **quick 模式只配自检**：criterion `--quick` 在纳秒级 bench 上抖动可达 ±100%（dispatch/1 实测 127→268ns）,基线与门禁正式判定必须全量采样;门禁脚本因此把透传明确定位为「快速自检」。

## 五、遗留

- bench 覆盖面可续扩（后续候选：truth 文件合并 `run_promotion_pass`、`normalize_text` 后校验、chapter index 序列化）——按「先有回归嫌疑再补 bench」节奏加,不为全量覆盖而覆盖。
- CI 侧无门禁（本仓已移除 GitHub Actions,发版走本地脚本）——bench:gate 属本地纪律,发版前建议与 duel 同跑。
- 总体方案剩余 backlog：W-A4b（secrets 掩码,需 UI 确认）、W-B4/B5（插件生态）、W-C3/C4（产品级）、en/ja README、TROUBLESHOOTING 复审。
- 推送须在 Fork 图形端执行（既有约定）。
