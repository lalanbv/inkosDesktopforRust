# 848 号：audit:npm 形状卫兵 + 必红自检——同型扫仓收口解析型门禁族

日期：2026-11-02。前置：847 号。846/847 的恒绿教训推广到其余解析型门禁的全仓扫仓轮。

## 一、活体发现（audit:npm 与 846 同型，且正在发生）

审查 `scripts/audit-npm.mjs`（467 号，与 audit-rust 同期同风格）发现：

```js
const advisories = Object.values(parsed.advisories ?? {});   // 键缺失被 ?? {} 静默当 0
if (advisories.length === 0) { console.log("✓ 0 advisory"); process.exit(0); }
```

**实测端点瞬态响应就是顶层 `{"error": ...}` 对象**（本轮首次探测即命中一次；复跑三次才稳定为完整 v1 形状 `[actions, advisories, muted, metadata]`，10 advisories）——即端点故障/限流/改形时旧代码报「✓ 0 advisory」**假绿**。846 轮 gate:ts 里 `audit:npm ✓ 3.6s`（异常地快）极可能已吃过此假绿。

## 二、实施

- **audit-npm.mjs**：①形状卫兵 `auditShapeFailure`——无 `advisories` 键 / null → **exit 3**（执行面失败≠审计通过），回显响应头 240 字节与「稍后重跑」指引；②metadata.vulnerabilities 汇总与 advisory 计数矛盾（0 vs 非空）→ exit 3；③分类重构为纯函数 `classifyAdvisories`，真审计与自检共用同一路径；④**三枚内联夹具必红自检**（未白名单告警→红 / 错误形状→红 / 全白名单→绿），默认跑在真审计后，`--self-check-only` 毫秒级独立；任一不符=检测器退化、本次绿灯结论作废（码 1）。
- **audit-semantic-patterns.mjs**：加零扫描面卫兵——SCAN_DIRS 存在但 0 源文件 → exit 2「Not a pass」（防目录结构漂移被误读为全绿；缺目录场景原本就以未处理拒绝响亮崩）。
- **复核无洞**：bench-gate.mjs 解析失败一律 exit 2（缺基线 exit 0 是显式首跑引导语义，基线入库删除可见）；clippy-gate / cargo-test-gate / gate-ts 均退出码型，无此类洞。engine-contract-diff 为双端活体互比（另一类，845 号已论证）。

## 三、验证

- audit-npm：语法 ✓；`--self-check-only` 3 夹具全 ✓ exit 0；**sed 注入恒 false 谓词 → ✗ exit 1**（自检失败路径真实非恒绿）；真实全路径 = 10 advisories 全白名单 + 自检 ✓ exit 0。
- audit-semantic-patterns：正常路径 42 candidates（信息型人审清单，未接线门禁）；错误 cwd → ENOENT 响亮崩 exit 1；空目录树 → 卫兵 exit 2。
- **gate:ts 八步全绿**（audit:npm 4.6s 含真审计 + 自检；全链接线实证）。
- 本轮零生产代码变更（2 脚本），testgate/契约无需重跑。

## 四、教训

1. **同一作者同一时期写的解析型门禁往往带同一类洞**——audit-rust（文本 grep+丢 stderr）与 audit-npm（JSON+`?? {}`）表面形态不同，根因相同：**解析失败/形状未识别时默认了「0」**。修复要按族扫，不按单个文件。
2. **瞬态错误响应是最危险的形状**：它合法地通过 JSON.parse，恰好落在「空 → 绿」分支。健壮判据是**正向确认**（必需键存在且非 null）而非**反向默认**（缺失即 0）。
3. 必红自检的三夹具设计：红例（检测得到坏）、错误形状例（解析得到怪）、绿例（不误报好）——三类都过才算检测器活着。

## 五、出口

维持：上游数值分协议扩展 / serde_json >1.0.154 解除 / @streamdown/math katex 0.18 / pi1.0 key / 新真实需求。**用户侧待办：Fork 推送本地累积 48+ 提交。**
