# 914 号 · 护栏常驻化轮：work 域交替读写互证入门禁（gate:ts 八步→九步，交替步 5.2s）

> 勘察定案：works 域双引擎交替读写互证（884 M1b 验收器械，33/33）
> **不在任何常驻门禁**——884 起仅在验收/定性轮手动跑，回归护栏半闭
> 环（913「器械不留用户文档」同型逻辑的工程面）。本轮接入 gate:ts 为
> 第 9 步 `work-alternation`，实测 **5.2s**（works 域零 LLM/mock 依赖
> +磁盘事务形态，成本极低护栏闭环）。
> 验证：gate:ts 九步全绿（build 16.8s/test 101.3s/work-alternation
> 5.2s）；零生产代码改动（门禁脚本单文件）。

## 一、勘察与裁决

- `scripts/work-domain-alternation.mjs`（884 号 M1b 验收）：同一项目
  根 Node→Rust→Node 交替读写 works/与 .inkos/harness.sqlite，33 项
  断言（Node 写读基线→Rust 整份读活→Rust adopt→Node zod 解析→归并
  读出→production-state 交替回读）。884 起仅手动轮跑（905/906 各一
  次），非验收轮不跑=works 域双引擎等价的**回归护栏缺位**。
- 裁决=入常驻：works 域零 LLM/mock 依赖（884 设计即零 mock 可跑）、
  磁盘事务形态、历史多轮全绿（稳定性风险低）；时长实测 5.2s（九步
  中最轻活体步）。与 913「修复不留用户文档=半闭环」同型：**器械不留
  门禁=护栏半闭环**。

## 二、落地（scripts/gate-ts.mjs 单文件）

- 步骤清单头注释补第 9 步说明（含 914 入驻理由）；steps 数组
  export-epub-smoke 后插 `["work-alternation", ["node",
  ["scripts/work-domain-alternation.mjs"]]]`——与 engine-contract-diff
  同族（双引擎活体）序列相邻。
- 九步全量验证：work-alternation ✓ 5.2s、全门禁通过。

## 三、验证汇总

- gate:ts **九步**全绿（新增步 5.2s；test 101.3s 同族）；U+FFFD 净
  （gate-ts.mjs 无中文新增段外全为既有内容，实测 0）。
- 零生产代码改动；clippy/testgate 与 Rust 面无涉（gate:ts rust-bin
  步活体核验在位，同日 907/909/913 连续复跑绿维持）。

## 四、后续出口

- 护栏常驻化收官：works 域双引擎等价从此随 gate:ts 每轮回归。
- 出口清空回归常备：上游协议扩展或外部事件（pi1.0 key）或新真实需求
  或下一巡检周期。

**下一号自 915 起；出口清空回归常备；用户侧待办=Fork 推送 48 个提交。**
