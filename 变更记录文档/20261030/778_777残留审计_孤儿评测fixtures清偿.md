# 778 号：777 残留审计收口——孤儿评测 fixtures 清偿（688 号静默删除漏网）

> 对 777 号（三面 ops 死字段清偿，commit 0afa983d）做全量复验审计：
> 双形态全仓 grep + 双探针点名 + 全门禁复跑。审计确认 777 本体完好，
> 但扫出一处 777 清理扫幅的漏网——`packages/core/src/__tests__/eval/
> eval-fixtures.ts`，且深挖后定性为 688 号上游合并静默删除审计的遗留
> 孤儿，本轮一并清偿。

## 一、777 本体审计结论（全部实证通过）

- 提交面：0afa983d 在 HEAD，工作树干净，9 文件 +147/−45 与归档一致；
- 残留面：snake_case（subplot_ops 等）在 engine-rs 全仓（含测试）零命中；
- 兼容锁面：leaf.json 残留 12 行三键全部位于 4 个用例的 `input.delta`
  输入侧（传输面兼容锁，刻意保留）；`settler_delta_parser.rs` 的
  `delta_json()` 夹具与容忍探针自身为同族保留面；
- 新增逻辑面：双探针点名复跑绿（模块 9 passed）——
  `legacy_panel_ops_keys_are_tolerated`（旧键容忍，防误加
  deny_unknown_fields）+ `minimal_new_form_delta_parses_without_panel_ops`
  （清偿未收窄解析接受面）。

## 二、发现与修复（2 文件，净删除）

**表象**：`eval-fixtures.ts:43-45` 仍携带 `"subplotOps"` 等三键——777 号
双形态清理 grep 的扫幅未含 `__tests__/eval` 目录。

**实质**（消费图判定，比表象深一层）：
1. 该文件是 369 号 R8 行为评测集的共享 fixtures（continuity/reviser/
   settler 三评测 + `pnpm eval` 脚本，4 文件）；688 号上游 2.0 合并按
   「上游删除的 1.x 模块/测试接受删除」裁决删掉了三评测测试，但 fixtures
   与 core package.json 的 `eval` 脚本漏出了静默删除审计——目录内自
   此只剩 fixtures 一个文件，零 import、零消费端；
2. `SAMPLE_SETTLEMENT_OUTPUT` 模拟的是 1.x 文本双节（`===
   RUNTIME_STATE_DELTA ===`）旧形态且携带三键；TS 2.0 基底的
   `RuntimeStateDeltaSchema` 是 **strict** 且无 ops 字段（形态为
   chapter/factOps/hookOps/newHookCandidates/chapterSummary）——若未来
   有人复活消费它，strict 解析对三键直接拒绝，是会咬人的陈旧重量而非
   兼容锁（与 leaf.json input 侧「Rust 容忍语义下的锁」有本质区别）；
3. core 的 `eval` 脚本指向空目录，vitest 无测试文件即非零退出——该脚本
   自 688 号起必红，却因不在任何门禁链（gate:ts 为 `pnpm -r` 家族，
   不含 eval）而无人发现。

**处置**：删整个孤儿文件（而非只删三行）+ 摘 `eval` 脚本。只删三行会
留下「fixture 模拟一种双栈当前都不存在的输出形态」的误导与坏死脚本；
评测集测的 1.x 确定性管线已随 2.0 退役（688 号裁决），移植评测集属
无消费端的新功能开发，不做。fork 自有文件删除无上游合并冲突面；
369/385 号归档与 backup/pre-upstream-2.0 分支保底历史。

## 三、验证

- engine-rs 双探针点名 + 模块全测：9 passed；
- `pnpm cargo:testgate`：engine-rs 46 目标 1947 passed / src-tauri
  25 目标 583 passed，双 crate audit 0——与 777 基线零漂移（本轮仅动
  TS 侧，符合预期）；
- `pnpm clippy:gate`：双 crate 0 告警；
- `pnpm gate:ts` 八步全绿（build/typecheck/test/audit:npm/rust-bin/
  node-fallback-smoke/engine-contract-diff/export-epub-smoke）——core
  vitest 全量（test 步 115.8s）与契约差分（0 分歧）在删除后照常通过；
- 终态残留复扫：两形态全仓仅剩上述刻意保留面。

## 四、教训

1. **静默删除审计的覆盖单位应是目录而非文件**：688 号合并审计按文件逐个
   定性，删测试时放过了同目录的共享 fixtures 与配套脚本。「孤儿文件用
   消费图判定」要在合并后按目录全扫一遍才闭环；
2. **清理类变更的收尾 grep 必须含测试夹具目录**：777 号双形态扫幅
   （src/prompt/golden/scripts）漏了 `__tests__/eval`——夹具里躺着的
   旧形态键不会红任何门禁，只有主动全仓 grep 能捞出；
3. **死脚本不入门禁也会腐烂**：`pnpm eval` 指向空目录必红半年无人知，
   因其悬在门禁链外——便利脚本要么进门禁要么删除，「留着但不跑」是最差
   形态。
