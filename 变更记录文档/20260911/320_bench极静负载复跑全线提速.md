# 320 号：bench:gate 极静负载复跑——九项基准全线优于基线（最快 -15.9%）

- 日期：2026-09-11
- 分支：develop
- 关联：272 号（安静窗复跑先例）、315 号（负载前置检查机制）
- 编号衔接：查 20260911 目录最大号 319，顺延 320
- 推送核验：origin/develop = 498253fa（300 号）未动；本地领先 301–320 共 20 提交待推

## 一、极静负载窗口复跑（loadavg 0.97/18 核 ≈ 5%/核——本会话最安静窗）

| 基准 | 基线 → 本次 | 变化 |
|---|---|---|
| chapter_index/parse_200 | 42.83 → 40.80 µs | -4.7% |
| paragraph_scan/detect_shape_3000zh | 1.81 → 1.76 µs | -3.0% |
| sensitive_words/scan_chapter | 16.66 → 15.72 µs | -5.7% |
| sensitive_words/scan_chapter_builtin_only | 9.60 → 9.11 µs | -5.1% |
| sse_broadcast/dispatch/1 | 142.15 → 119.56 ns | **-15.9%** |
| sse_broadcast/dispatch/32 | 126.73 → 121.91 ns | -3.8% |
| sse_broadcast/dispatch/8 | 126.39 → 119.50 ns | -5.4% |
| title_dedup/resolve_duplicate_hit_200 | 432.54 → 406.60 µs | -6.0% |
| title_dedup/resolve_no_duplicate_200 | 61.05 → 56.64 µs | -7.2% |

**九项全部快于基线**（无一项回归），门禁 exit 0。基线文件未触碰（严禁 --update 洗基线纪律）。

## 二、结论

301–319 号的全部代码变更（body 上限重构/arcs 形状/意图路由/依赖升级）对热路径**零性能回归且有改善**——性能面在最新代码状态下优于 259 号基线采集时的水平。

## 三、验证性质与遗留

零代码改动批（纯复跑）。遗留：301–320 共 20 提交待推送；并行会话三件第四十六轮在途未落库；两项默认值、ja A/B、历史瘦身待用户决策。
