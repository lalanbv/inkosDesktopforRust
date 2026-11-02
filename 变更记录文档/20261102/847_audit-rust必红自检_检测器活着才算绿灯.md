# 847 号：audit:rust 必红自检落地——检测器活着才算绿灯

日期：2026-11-02。前置：846 号。846 号教训 #1（「解析型门禁应配必红用例」）从教训落地为机制。

## 一、巡检快检（廉价项）

上游 `fetch --prune` 仍 **0**；advisory-db 当日（846 号刷新）。serde_json 与 bench 在 846 号（约一小时前）刚验证且本轮零 Rust 代码变更，按「巡检粒度随发现递减而放宽」不重复跑。

## 二、实施

- **夹具 `scripts/fixtures/audit-rust/known-vulnerable.Cargo.lock`**：最小锁（仅 name+version+source 两包，cargo-audit 按 name+version 匹配无需依赖图），钉 `zip 2.6.1`（RUSTSEC-2026-0367/0368/0369/0429）+ `smallvec 0.6.9`（RUSTSEC-2019-0009/0012/2021-0003）——两组独立防单条 advisory 被撤回转绿。文件头明示**永远不该被"修复"**。
- **`audit-rust.mjs` 抽 `runAudit/parseAudit` 共用路径**：真审计与自检走同一段解析代码——自检证明的就是这段代码还能看见红。
- **`--self-check` 机制**（默认启用、跑在真审计之后 DB 已就绪）：`cargo audit -n --no-yanked -f 夹具`（离线秒级）断言 vulns≥1 且摘出 ≥1 条 `ID: RUSTSEC-*`；失败即判定检测器退化（846 号事故同型），**本次所有「0 漏洞」结论作废**（码 1）。`--self-check-only` 供秒级单独自证。
- **`INKOS_AUDIT_SELFCHECK_FIXTURE` 环境覆盖**：仅供测试自检本身（喂干净锁证明自检会失败），生产不设。

## 三、实证（六场景）

| 场景 | 结果 |
|---|---|
| `--self-check-only`（夹具） | ✓ 检出 7 漏洞 / 7 条 RUSTSEC，exit 0 |
| 覆盖夹具=干净锁（engine-rs 当前锁） | **✗ exit 1**「未报红…结论作废」——自检自身非恒绿的必红证明 |
| 覆盖夹具=不存在 | ✗ exit 1「夹具缺失」 |
| 夹具缺 `source=registry+…` 行（847 过程实测） | **cargo-audit 视为本地 path 包直接跳过 → 0 漏洞**——夹具自身曾恒绿，已修正并写入夹具注释 |
| 默认全路径（双 crate 真审计 + 自检） | ✓✓ + 自检 ✓，exit 0 |
| testgate 审计步（无参调用） | 自动含自检，无需改 cargo-test-gate.mjs |

## 四、勘误与归因修正

- 846 号归档/记忆称「ustc 宕机是 yank 核验超时来源」**不成立**：本轮 ustc 已恢复（200/0.16s）而 yank 超时依旧 315/758。实测流内顺序=「Updating crates.io index」后逐 crate 查询——**本网络到 index.crates.io 延迟 ~1.6s 远超 cargo-audit 极短每请求窗口**所致，与 ustc 无关；属折叠噪声、门禁中性。记忆已勘误。
- 夹具无 `source` 行时恒绿的教训单列：**自检的输入本身也需要必红证明**——绕过循环的方式是让自检支持注入错误输入（环境覆盖），每类失败模式各验一次。

## 五、门禁

`node --check` 语法 ✓；六场景全过（见上表）。本轮零生产代码变更（1 脚本 + 1 夹具），无需重跑 testgate/契约；审计步形态已在默认全路径中实证。

## 六、出口

维持：上游数值分协议扩展 / serde_json >1.0.154 解除 / @streamdown/math katex 0.18 / pi1.0 key / 新真实需求。**用户侧待办：Fork 推送本地累积 47+ 提交。**
