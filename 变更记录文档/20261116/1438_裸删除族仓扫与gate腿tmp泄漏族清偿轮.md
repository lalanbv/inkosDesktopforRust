# 1438 号：rmSync 裸调用族仓扫 + gate 腿 tmp 泄漏族清偿轮

## 目标与范围

/goal 逐功能测试第十轮增量。1437 修复面=纯注释（行为恒等、当轮已活体）不再重
复走查；本轮非冗余增量=**846-848「修洞按族扫」纪律补 1436 单点修复的同族仓
扫**（裸 rmSync 清理路径族）+ **1435 tmp 泄漏族的全前缀普查扩展**（1435 普查
只扫 `cargo-ignored-sweep-*` 前缀，属前缀 scoped 盲区）。

## 表面同一性（门禁谱系继承论证）

`git diff 89508d33..HEAD --stat`：1435 全量门禁后至 1437 HEAD 仅
sweep 脚本（24 行）+ 两份记录文档——**零生产代码（Rust/TS/packages）增量**，
1435 十腿/audit 族/bench 谱系对当前树可继承。

## 族扫发现（scripts/ 27 个 Node 门禁脚本全扫）

**伪红族（裸 rmSync 清理路径）三处真命中**：

| 站点 | 崩形 |
|---|---|
| engine-contract-diff `:2189`（finally） | 绿判后 finally 内 throw → 未捕获崩 exit 1 = **绿→伪红**（gate:ts 十腿之一） |
| work-domain-alternation `:625`（finally） | throw 崩在 `failed` 汇总**之前**=同伪红 + 「临时根已清理」讯息撒谎（gate:ts 十腿之一） |
| work-domain-alternation `:259`（红档早退） | exitCode 已 1，崩后未捕获退出码同 1 不翻判——仅栈噪声，家族收敛性修复 |

review-cycle-matrix `:206` 已 try/catch 守卫（负结果亦是结果，980 先例）。

**泄漏族（1435 全前缀普查扩展）五高量机械族 ≈597 对象**：

| 前缀 | 存量 | 写入者 |
|---|---|---|
| `inkos-epub-smoke-*` | 78 | export-epub-smoke.mjs（每腿一根零清理） |
| `inkos-smoke-*` | 75 | node-fallback-smoke.mjs（同上） |
| `inkos-epub-entity-*.epub` | 152 | translation-ingest.test.ts 直写 tmpdir 根（每跑两枚之一） |
| `inkos-epub-xxe-*.epub` | 152 | 同上（另一枚） |
| `inkos-tui-single-*` | 140 | tui-session-store.test.ts per-it 根零清理 |
| `inkos-tui-session-*` | 1 | 同文件 afterAll 明写 `// no cleanup needed, tmpdir`（tmp 判免费谬误，1435 已定性为泄漏族的注释面实例） |

## 修复（六文件，98+/12-）

1. **ecd `:2189` + wda `:625`/`:259`**：try/catch 非致命化+「清理失败，保留现
   场：<路径>」如实入讯息（cargo-ignored-sweep 1436 同族惯用形）。
2. **两 smoke 清根**：`legRoots` 台账+**绿档即删/红档保留现场**（1435 sweep 同
   族语义）+`--keep` 卫兵——node-fallback 头注释本就承诺「--keep 保留服务器与
   临时根」（既有承诺兑现）；export-epub 的 `keep` 旗标解析后从未接线（死旗
   标），同语义接线+用法注释补 `--keep` 文档。
3. **translation-ingest.test.ts**：epub 样例收编 per-test mkdtemp 根（次
   describe 同款惯用形，afterEach 清盘），去掉 Date.now() 命名碰撞面。
4. **tui-session-store.test.ts**：afterAll 真清盘（替换谬误注释）+per-it 根
   try/finally 清理+补 `rm` 导入。

## 设计自纠一处（当轮活体抓到）

nfs 保留条件初版含 `softFailCount > 0`——**700 号软失败=记账非判定**，当前
2.0 双栈基线下每跑必现（`/prompt-packs` 404 ⇒ softFailMode）→ 该条件会让泄漏
换个借口永续（首跑 tmp-delta=2 实证）。收窄到 `failures > 0`，软失败腿按绿档
清、现场走 `--keep`。复跑 delta=0。

## 活体矩阵（全当轮实证）

- **四腿绿+tmp-delta=0**：work-alternation 33/33（「临时根已清理」如实讯息活
  体）/ export-epub 双腿 / node-fallback 双腿（修正后复跑）/ engine-contract-diff；
- **vitest 两文件**：translation-ingest 6/6 + tui-session-store 4/4，泄漏
  delta=0（epub 族此前每跑 +2）；
- **双包 typecheck 过**（tsc --noEmit 静默）+ node --check 四脚本过；
- **存量清盘**：五族 739 对象（TMPDIR inkos-* 总量 3562→2823），五前缀全零
  （loud no-match 确认）。

## 残量地图（1439 起点，本轮不扩线）

TMPDIR 残 2823 条 inkos-*：`inkos-book-backup-`×1400（writer=book-backup.test.ts）/
`inkos-plan-`×608（persisted-governed-plan + request-delivery.flow 的
`inkos-plan-before-draft-`）/`inkos-chapter-cmd-`×~790（writer 待定谳）/小族
（short×7、play-edit×6、play-image×3、m1-demo×1=supervisor 用例设计内、
diff-mock.log×1=固定名覆写）——同机械族，下轮逐写入者清偿。

## 0GC/性能面与不引入新问题论证

全部冷路径门禁脚本+测试面，零热路径消费；新增循环=腿根数级（≤2）。ecd/wda
成功路径行为不变（腿活体当轮实证）；smoke 清根只增不改失败语义（--keep/红档
保留/软失败记账均守恒）；vitest 改动仅资产重定位+清理（6/6+4/4 绿）；终态零
孤儿/零监听。U+FFFD 六文件净。

出口=下一巡检周期/earendil-mz 单包合并；用户侧待办=Fork 推送(含 955-1438)+
正式打包三脚本重烤。下一号自 1439 起。
