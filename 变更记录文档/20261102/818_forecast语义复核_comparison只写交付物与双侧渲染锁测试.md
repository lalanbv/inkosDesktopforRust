# 818 号：forecast 语义复核——comparison.md 只写交付物定性 + 双侧渲染锁测试

日期：2026-11-02。前置：816 号登记（forecast 渲染副本「换行→空格」语义复核，择机项）。

## 一、巡检快检

- 上游 `fetch --prune` 复跑：`master..upstream/master` 仍 **0**（817 号零落后维持，当轮后无新推送）。
- `origin/development`：master 领先 14（=待推提交 804-817），development 独有 0——**纯快进**，无分叉无工项（上游遗留的第二分支，不删不代推）。
- RustSec/npm audit：本日 testgate/gate:ts 多次全绿（0 漏洞）。

## 二、forecast 语义复核（登记项清偿）

- **comparison.md 是只写交付物**：写入 `story/runtime/narrative-forecasts/<id>/comparison.md` 后仅以路径被引用（runner/forecast-tools 报告 comparisonPath），**全仓无任何回读解析**——812 族的「回读拆列」问题在此面不存在；真正的数据回读走 `forecast.json`（结构化 schema.parse，与单元格转义无关）。
- 双侧 `escapeCell` 逐字同集：`|`→`\|`、`\n`→空格；连 CRLF 残留裸 `\r` 的 wart 也一致（TS `/\n/g` 与 rust `'\n'` 同样不处理 `\r\n` 前半）——markdown 渲染中 `\r` 视同空白，视觉无差。
- **呈现差异（无契约面）**：rust 对比表含「意图匹配」列（`intent_alignment.score`，fork 产品字段），TS schema 只有 `intentRationale` 无分值故 TS 表无该列——只写件的用户可见差异，数据由各自 schema 界定，不对齐。

## 三、锁测试（复核交付出物）

双侧此前**零测试**覆盖 forecast 渲染转义：

- rust `forecast/render.rs::tests::comparison_table_escapes_pipes_and_newlines_in_cell`：title `合作|对抗\n背水一战` → 表格行恰一行且为 `| branch-1 | 合作\|对抗 背水一战 | 80 | 0 | p |`（分支小节标题含裸竖线合法，不属表格——首轮断言过宽已收窄）。
- TS `__tests__/forecast-render.test.ts`：同输入镜像断言（`| branch-1 | 合作\|对抗 背水一战 | 0 | p |` 单行；TS schema 无意图列故 `0` 为风险数位）。

## 四、验证

cargo:testgate engine-rs 47 目标 **1980**（+1）+ src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core **180/988**（+1 文件 +1 测）+ studio 137/725 + cli 23/81。diff 71/1 维持。

## 五、登记（695 工项余项）

- 全部余项收敛为：**数值分协议扩展（上游面）**——overallScore/averageScore/scoredChapters 与 issueCount 值差同源；其余登记项（forecast 复核/渲染副本合并/TS 计数口径/rust 重建去重/UI 徽标/mock 记录工具支）均已清偿或定性关闭。
- 出口维持：上游协议扩展 / pi1.0 key / 新真实需求 / 下一巡检周期（upstream remote 留存）。

## 六、教训

1. **「另语义副本」的处置是核实并记录边界，不是合并**：forecast 的换行→空格与正文台账的换行→`<br>` 是两个刻意的呈现语义——复核产出是"为何不同"的边界说明（只写交付物 vs 回读投影）而非统一。
2. 锁测试断言要贴合渲染事实：分支小节标题合法含裸竖线，全局 `contains` 断言会误伤——**先跑一遍看真实输出再写断言**。
3. 双侧「同 wart」（CRLF 残留）也是对等性证据：连缺陷都逐字一致时，可判定为同源拷贝而非漂移。
