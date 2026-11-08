# 931 号 · Rust 基准活体首跑轮：hot_paths 双端同名族数值对照——移植无性能回退的活体证据

> goal「最佳高性能」的 Rust 维度运行态观察点首轮：hot_paths.rs（criterion
> 0.5）活体首跑成功（325s，read_chain 七面+其他族）。**新证据面=双端
> 同名族数值对照**：Rust 与 TS bench（bench-baseline.json）同名读链族
> 数值惊人接近（µs 级差 1~5%）——「TS→Rust 移植无性能回退」的活体实
> 证（此前仅功能等价面有差分器证据，性能等价面首次有数）。
> 验证：bench 首跑通过；gate:ts 十步全绿（当日 914-931 十一轮连绿）；
> 零代码改动轮。

## 一、Rust 基准首跑（hot_paths 活体）

- criterion 100 样本形态；read_chain 七面数值（µs）：
  book_detail 651 / chapter_workspace 504 / analytics 460 / truth_list
  149 / list_genres 140 / read_chapter 50.6（+list_books 等早段族）——
  引擎读链全族 µs 级健康。
- criterion 自带 target/criterion 历史目录=后续运行的 regression 对比
  基线（观察点已建立）。

## 二、双端同名族数值对照（新证据面）

| 读链族 | TS（µs，bench-baseline） | Rust（µs，本轮） | 差 |
|---|---|---|---|
| book_detail | 665 | 651 | −2% |
| read_chapter | 49.0 | 50.6 | +3% |
| analytics | 463 | 460 | −0.6% |
| truth_list | 150 | 149 | −0.7% |
| list_genres | 144 | 140 | −3% |

- **移植无性能回退的活体证据**：差分器此前证功能等价（72/0），本轮
  性能等价首次有数（µs 级 1~5% 同水准）——「TS→Rust 移植质量」维度
  补全性能面。

## 三、验证

- bench 首跑通过（325s）；gate:ts 十步全绿（当日十一轮连绿）；
  零代码改动（基准运行+对照观察，无基线文件改动——TS bench-baseline
  与 Rust criterion 各自历史自持）。

## 四、后续出口

- Rust 基准观察点已建立（后续巡检周期可跑对照 regression 报告）。
- 出口清空回归常备。

**下一号自 932 起；出口清空回归常备；用户侧待办=Fork 推送 62 个提交。**
