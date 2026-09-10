# 272 号：推送核验全清+268–271 交叉评审+bench:gate 安静窗全量绿

- 日期：2026-09-10
- 分支：develop
- 关联：268–271 号（并行会话批次——本批交叉评审对象）、259 号（bench 基线——本批隔离复跑确认）、260/261 号（交叉评审先例）
- 编号衔接：查当日目录最大号 271，顺延 272
- 推送核验：**origin/develop = develop = d26658b1，领先 0**——用户已在 Fork 图形端完成推送，201–271 号全部上远端；此前自 232 号起多轮记录的「待推送」阻塞项解除

## 一、268–271 号交叉评审（并行会话，只读未触碰其在途代码）

事实声明逐条独立核验，全部属实：

| 声明 | 核验方式 | 结果 |
|---|---|---|
| 268/271 号：`scripts/studio-e2e-benchmark.mjs` 已被上游 a467d5c1 删除、入口残留 | `ls` + git log | ✓ 文件确不存在 |
| 271 号：core/cli/studio 三包均无 lint 脚本，`pnpm -r lint` 死路 | grep 三包 package.json | ✓ 零命中 |
| 268 号：rustbin.rs release 专属警告修法（下划线参数，debug 块内引用合法） | 读 diff | ✓ 惯用修法，调用方无感 |

**一处边界提醒（待其收尾时自行斟酌，不代改）**：`audit-rust.mjs` 设计上丢弃 stderr（yanked 联网噪声）+ 按 `error: N vulnerabilities found` 正则解析——若 cargo-audit 因**运行期故障**（如 advisory db 拉取失败）非零退出，stdout 无匹配会被解析为「0 漏洞」绿灯。可考虑校验成功标记（如 `Cargo Audit` 头或 `Success No vulnerable packages found`），两者皆无则按审计失败处理——与其「审计缺失不静默绿灯」的设计原则对齐。

## 二、bench:gate 全量门禁（安静窗口，259 号遗留清偿）

- 前置：1 分钟 loadavg 2.74/18 核 ≈ 15%/核，低于 20% 拒绝线，门禁放行（此前数轮 72%/核被正确拒绝，见 265/267 号）
- 结果：**exit 0，9 项基准全部通过**（阈值 +30%）：

```
chapter_index/parse_200                    +1.9%
paragraph_scan/detect_shape_3000zh         +5.4%
sensitive_words/scan_chapter               +4.4%
sensitive_words/scan_chapter_builtin_only  +4.2%
sse_broadcast/dispatch/1                   -8.8%
sse_broadcast/dispatch/32                  +2.2%
sse_broadcast/dispatch/8                   +3.0%
title_dedup/resolve_duplicate_hit_200      +1.2%
title_dedup/resolve_no_duplicate_200       +0.0%
```

- 漂移区间 -8.8%~+5.4%，与 257 号实测的负载噪声形态（+190%~+520%、逐轮漂移基准各不同）完全不同量级——属正常机器态方差。**259 号基线继续有效，未触碰基线文件**（严禁 --update 洗基线纪律）。

## 三、验证

本批纯评审+复跑，零代码改动。cargo 面门禁状态：engine-rs/src-tauri 全量 test+clippy 于 263/266 号绿且其后无 Rust 代码合入（并行会话 rustbin.rs 改动尚在途）；Node 面 test/typecheck 于 265/271 号绿。

## 四、遗留

1. audit-rust.mjs 运行期失败误报绿灯的边界（第一节）——并行会话在途，留其处置。
2. 两项默认值、ja A/B、历史瘦身——待用户决策（承接 264/265 号，推送一项已清）。
