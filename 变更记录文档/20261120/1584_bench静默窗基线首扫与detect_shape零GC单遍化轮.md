# 1584 bench 静默窗基线首扫 + detect_shape 零 GC 单遍化轮（2026-11-20）

## bench:gate 五轮备案兑现（1448 号静默窗口首次满足）

凌晨 1:49 loadavg 1min 14%/5min 10%（1579 以来 32%→24%→28%→22%→**14%** 回落终点），**wasmtime 36→50+工具链 1.94→1.100 后热路径基线首次复扫**。结果：21+ 基准整体带内，**1 个超阈回退**——`paragraph_scan/detect_shape_3000zh: 1.72µs → 2.80µs（+63.3%）`。

## 回退归因链（代码/依赖零变化 → 工具链变量 → 本底反模式）

1. 隔离复跑 criterion 30 采样稳定 2.615µs（置信区间 [2.613,2.620] p<0.05）——真实回退非噪声；
2. `git diff HEAD~1`：post_write_validator.rs 零改动（最后改动=1487 fmt）；engine-rs/Cargo.lock 仅删 anyhow 7 行（依赖面零变化）；基线锁于 785 号（collectedAt 2026-11-01=工具链 1.94+wasmtime 36 时代）；
3. 定性=**工具链 1.100 代码生成差异使本底分配反模式成本显性化**（1.94 编译器内联部分掩盖）。

## 0GC 单遍化修复（真 0GC 而非刷基线）

analyze_paragraph_shape 原形态反模式：utf16_len **三遍扫描**（short 过滤/average 求和/连续短段循环）+ 短段/段落两处**全量 String clone**——而消费面只有计数（`paragraphs.len()`/`short_paragraphs.len()`，ParagraphShape 私有 struct 文件内消费 grep 实证：detect_paragraph_shape_warnings + detect_paragraph_length_drift 两消费方全为 `.len()`）。

修复=regex split 迭代借用（零 collect 零 to_string）+ 单遍循环（一次 utf16_len 同时算 short 判定/short_count/narrative_count/total_utf16/max_consecutive）。**语义保持锚点**（审计三则）：①max_consecutive 只看叙述段序列——对话段 skip 且**不重置**连续计数（narrative_paragraphs 过滤后迭代的原语义：对话段在序列中不可见，两短叙述段隔对话段仍连续）；②average 分母=全部段落含对话段（total_utf16 对话段也累加）；③filter 链合取顺序无关。extract_paragraphs 唯一消费点即本函数——随之删除（Vec<String> 中转层消失）。

**验证**：post_write_validator 23/23 单测全绿（行为锁定面）+ bench 复跑 **935ns**（vs 回退 2.6µs **-64%**、vs 旧基线 1.72µs **-46%** 超越，p=0.00）——1.94 时代 1.72µs 里大部分成本即三次扫描+clone（编译器只掩盖部分），真 0GC 砍掉本底。红路四拍：红=bench-gate 实跑 +63.3%（真跑浮出）→归因→0GC 修复→绿=935ns 超越旧基线。

## 基线刷新（--update 全量，785 号「如实记录已发布现实」先例）

wasmtime 50+工具链 1.100 是**环境变化**（基线同机同环境才可比，bench-gate 文档明示）+ detect_shape 0GC 优化收益——两者都该进新基线。静默窗全量 `--update` 刷新（collectedAt 2026-11-19T18:07 UTC），刷新后 bench:gate 复跑 ✓ 通过。

## 验证

- cargo:testgate 双 crate **2729 passed**（2129+600 与 1581/1583 基线精确持平）；clippy/fmt 双 0；gate:ts 十腿全绿；U+FFFD 净。
- bench:gate ✓（新基线 22 场景锁定）。

## 后续

- 下号自 1585 起；用户侧待办不变（Fork 推送 955-1584 + 正式打包三脚本重烤+wasmtime 50 后 e2e_startup 插件面复核）。
