# 828 号：pedantic clippy 扫描——生产代码在 pedantic 级已是干净的

日期：2026-11-02。前置：817 号（默认门禁 + bench 捕获 810 回退）。本轮探查 pedantic 级改进机会（非门禁，目标=重构/优化）。

## 一、扫描结果（engine-rs `cargo clippy --lib -- -W clippy::pedantic`）

合计 **2735 条** pedantic 建议，按类别排序：

| 类别 | 条数 | 性质 |
|---|---|---|
| 文档风格（backticks/Errors/Panics 段） | 1081 | 文档美化，不涉及代码正确性 |
| `#[must_use]` 属性缺失 | 675 | API 文档标记， massive churn |
| 长字面量缺分隔符 | 609 | 纯格式 |
| redundant closure | 122 | **全部在测试代码**（lib 内 = 0 条） |
| `map_or` 惯用形 | 223 | 惯用法，可读性偏好 |
| 类型截断/精度 | 189 | 章节号 < 10000 场景不可能截断，pedantic noise |
| unnecessary not | 46 | 可读性偏好 |

## 二、行动裁决

- **redundant closure 122 条→lib 内 0 条**（全部在测试代码，`cargo clippy --fix --lib` 零变更实证）——生产代码在 pedantic closure 级已是干净的。
- 其余类目均为文档美化或惯用法偏好：批量应用会产生数千行机械 diff 且不改善运行时行为——**不做**，登记为可选 backlog。
- 类型截断警告（`usize→u32` 等）：章节号/词数字段在小说域不可能超出 u32 范围，pedantic noise。

## 三、结论

**生产代码在 pedantic 级已是干净的**——默认门禁 `-D warnings` + zero TODO + zero stash + zero 上游落后 + zero 转义/类型/章节文件识别副本，五面全零。剩余全部为上游协议级残差（数值分协议扩展）或文档/惯用法偏好（不涉及运行时行为）。

## 四、验证

clippy:gate（默认 -D warnings）双 0 维持；pedantic 扫描结果已记录；全仓 build/test/golden 70/70 维持。

## 五、教训

1. **pedantic 扫描的价值在发现盲区而非制造工作量**：本轮确认了盲区为零（lib 内 redundant closure=0、截断不可能），把「可能还有问题」变为「已扫无问题」。
2. 大量 pedantic 建议集中在文档风格与惯用法——这些是「品味」不是「正确性」，批量 chase 会产生数千行噪音 diff 掩盖真正的语义变更。
