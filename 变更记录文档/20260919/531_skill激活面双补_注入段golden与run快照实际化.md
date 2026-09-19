# 531 号：skill 激活面双补——注入段格式 golden 锁死 + run 快照 skillIds 实际化

日期：2026-09-19
提交：本文件同批路径限定提交
类型：test(golden) + fix(run-log 留痕诚实性)，双端对称

## 选题（530 号备案双候选一并清偿）

1. **写作链 golden 补 chat 出口注入段形态**：530 号新增的链级注入依赖"append
   拼接格式双端码点一致"，此前只有各自单测断言（TS 无专门断言、Rust 断言关键词
   非全文）——按 528 号机制补 golden 交叉锁定；
2. **skill 激活 run-log 留痕面**：侦查发现双端同构缺陷——写作链 run 快照
   （`story/runtime/chapter-NNNN.run.json`）的 skillIds **硬编码**"应当激活"
   （TS runner.ts `skillIds: ["inkos-long-writing"]`、Rust write_next.rs 三处
   `skill_ids: Some(vec!["inkos-long-writing"])`），530 号链级激活可空（builtin
   缺失/无技能部署 → 无注入）后快照与实际注入脱节——留痕不诚实。

## 改动

### golden：append 格式码点锁死（16 → 18 键）

- `golden-writing-prompts.test.ts`：+`skill.guidance.plain`（单技能/无资源/无既有
  system → 指导段前置为首条 system）+`skill.guidance.full`（双技能 + 两条参考资源
  含 heading 有/无对照 + 空 body 回退 description + 既有 system 追加形态）；
- `golden_writing_prompts_diff.rs`：Rust `append_activated_skill_guidance` 镜像断言；
  **首跑即绿**——TS base.ts 与 Rust agents/mod.rs 两份实现逐字同构获得交叉证实；
- 覆盖分支：body.trim() 空回退 description、`#### Reference: path:start-end · heading`
  （heading 缺省无后缀）、`\n\n` 连接、system 追加 vs 前置。

### run 快照 skillIds 实际化（双端对称）

- TS `_writeNextChapterLocked` baseRun：`skillIds: config.activatedSkills?.map(...) ?? []`
  （未配置激活 → 空数组如实记录）；
- Rust write_next.rs：+`snapshot_skill_ids()`（scope 内 task-local 实际激活的 id 表；
  无 scope → None→serde skip 键省略），三点快照（running/终态/失败）全部替换；
- 测试：TS 1460 断言同步（无激活 → `skillIds: []`）、530 透传测试补 run.skillIds
  断言（有激活 → `["inkos-long-writing"]`）、Rust +snapshot_skill_ids 单测
  （scope 外 None / scope 内 id 表）、sub136 e2e 断言同步（直调无 scope → 键省略，
  首跑红被抓后修正——老断言 `run["skillIds"][0]` 索引 null panic）。

## 门禁插曲

- **并行负载连锁**：gate:ts 后台运行时同机并行跑 cargo/vitest，vitest worker 超时
  连锁（test 步骤 1560s 失败、无失败用例输出）；隔离复跑 74.6s 全绿。教训：
  **gate 运行期禁止同机并行重负载任务**——与 bench 负载守卫同源的资源竞争问题在
  vitest 侧没有守卫，只会以假失败呈现；
- sub136 e2e 红 = skillIds 实际化的语义正确结果（无 scope → 键省略），断言按新
  契约更新，scope 内形态由单测覆盖。

## 门禁（8 项全绿）

| 门禁 | 结果 |
| --- | --- |
| cargo test engine-rs | 1804 passed / 0 failed（+1 新单测；sub136 断言同步） |
| cargo test src-tauri | 578 passed / 0 failed |
| clippy:gate（--all-targets 双 crate） | 0 告警 |
| gate:ts:fast | typecheck 17.1s + test 74.6s + audit:npm 全绿（core dist 先行重建） |
| 契约差分器（活体） | 41 对照 0 分歧 |
| node-fallback-smoke | 双引擎一致性全部通过 |
| export-epub-smoke | EPUB 结构校验通过 |
| INKOS_DUEL=1 strangler_duel | 10/10 真跑 |
| bench:gate | loadavg 3.58（19.9%/核）窗口通过，9 基准零回退 |

## 教训

- **留痕字段要记"实际发生"而非"应当发生"**：硬编码 skillIds 在激活可为空的架构下
  必然失真；530 号把激活变成可空后，同号顺手实际化留痕会更完整（两号拆分时第二轮
  要主动扫第一轮引入的语义依赖面）；
- 双端"同构缺陷"要一次对称修复（TS []/Rust None 语义各自对齐本地形态），差分器
  工件对照不含 run.json（525 号 10 列表），形态差异不会被抓——纪律性对齐不能依赖
  差分器覆盖面；
- gate 是串行承诺：gate 后台跑 + 同机并行测试 = worker 超时假失败，白跑 26 分钟。

## 下一号

自 532 起（开号前双查当日目录最大号）。备案候选：skill 激活的 use_skill 工具面
Rust 移植现状走查（139 号后演进核对）；写作链 golden 覆盖 writer prompt pack 层
（withPromptPackGuidance）另案评估。
