# 1579 detector 错误臂补测 + bench 窗口备案轮（2026-11-19）

## 定性

按 /goal 逐功能测试：1575 的 detection 链错误面只有**传输错误臂**覆盖（925 号挂起端点窗内报错），HTTP 层错误臂（`status>=400 → DetectError::Http` 分支）零覆盖——补行为级测试。bench:gate 复扫（1569 后首跑）被负载卫兵正确拒绝，定性备案。

## 补测（1 用例，无生产代码变更）

**`engine-rs/src/agents/detector.rs`** `detect_gptzero_maps_5xx_to_http_error_with_status`：mock 回 500+文本体 → 断言产出 `DetectError::Http { provider: "GPTZero", status: 500, body: "upstream boom" }` 三元组完整——5xx 不得静默走解析分支产出 score=0 的「判过」假结果（该假结果会让检测段直接放行，是检测契约的安全面）。detector 单测 9/9 绿。

## bench:gate 复扫备案（未跑成，卫兵正确拒绝）

`pnpm bench:gate`（1569 号后首跑，1572-1578 七轮修复后的热路径回归实证）：负载卫兵拒绝——1 分钟 loadavg 5.72/18 核=32% > 20% 阈值。负载源定性：**用户侧稳态活动**（Edge Helper 59.6% + ZCode UI Renderer 53.7%+21%，1/5/15 分钟同值=非瞬态），按 1448 号结论静默窗口只能由用户侧发起；`--ignore-load` 强跑在 257 号实测 ±190%~520% 噪声下判定不可信且违背禁洗基线纪律，不采用。**负载卫兵正确拒绝本身即防线活体实证**。热路径复扫留待静默窗口（用户侧待办备案）；回归风险定性低——1572-1578 修复面全在网络客户端构造层（回环谓词/客户端构造/错误臂），未触碰差分器/RRF/写法分析等算法热路径。

## 验证

- cargo:testgate 双 crate **2726 passed**（engine-rs 56 目标 2127 + src-tauri 599，+1 新用例）；clippy 0；fmt 0（cargo fmt 清偿 2 处折行）；sweep 5 腿绿（4567 净，1578 漏杀教训后首验 `lsof LISTEN` 归零）；U+FFFD 净。

## 后续

- 下号自 1580 起；用户侧待办追加：静默时段跑 `pnpm bench:gate`（1569 后首跑热路径复扫）+ Fork 推送（955-1579）+ 正式打包三脚本重烤。
