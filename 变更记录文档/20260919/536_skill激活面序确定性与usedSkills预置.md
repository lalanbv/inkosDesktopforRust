# 536 号：skill 激活面序确定性与 usedSkills 预置——532 号移植三缺陷清偿

日期：2026-09-19
提交：本文件同批路径限定提交
类型：fix(engine)，双端一致性（TS 为参照，零 TS 改动）

## 选题

会话目标「全量测试/代码审查」的延续：我方全量门禁矩阵（对 531 HEAD）之后，
并行会话同日入库 532/534/535 三笔——复审这三笔。结论：

- **534 号**（use_skill 检索项段级元数据）：实现干净，唯一瑕疵=代码注释 3 处
  写「533 号」（该号提交前撞号改 534，注释未随之改，未来 git 考古会误导到
  radar 修复）——本批顺手清偿；
- **535 号**（studio use_skill 工具卡激活预览）：审查通过零缺陷（防御式解析
  完备、正确消费 534 的段级元数据、use_skill 入 pipeline 白名单合理）；
- **532 号**（use_skill 激活注入闭环）：发现**三缺陷**，全部同根源=容器序语义
  未对齐，本批清偿。

## 532 号三缺陷（对照 TS 参照）

1. **turnSkills 缺 usedSkills 预置**：TS agent-session.ts:1147-1149 轮起点
   `turnSkills = new Map(usedSkills.map(...))`（resources 空），use_skill 仅
   动态叠加；Rust `scope_turn_skills` 起点恒空。后果：非 free-text 携带
   requested_skills 的轮次（resolution=None 路径），sub_agent 合并注入
   （worker ∪ turnSkills）丢失 usedSkills——TS 注入而 Rust 不注入。
2. **TURN_SKILLS 用 HashMap**：`turn_skill_activations()` 按 `values()` 随机序
   收集，而 TS Map 首插序。后果：同轮多技能激活时注入顺序双端漂移且 Rust
   自身逐轮不稳定——golden 纪律所锁的 prompt 码点序分歧。**532 号原测试的
   双键序断言在 HashMap 下本属掷硬币，能绿纯靠运气。**
3. **resolve_skills 双处随机序**（skills/mod.rs，532 之前即存在、sub_agent
   合并消费面放大）：`used: HashMap` → `used_skills` 随机序（TS registry.ts
   `used=new Map` → 请求序）；`disabled_skill_ids` 从 HashSet 迭代随机序
   （TS `[...new Set]` 插入序）。usedSkills 序直接决定 system prompt 指导段
   顺序（agent-session.ts:1258 for-of）。

## 改动（四文件，Rust 单侧）

- `skills/mod.rs`：`resolve_skills`——`disabled_skill_ids` 改输入序投影
  （normalize→滤空→去重→在册过滤，镜像 TS normalizeIdList+Set 序）；
  `used` 改 `Vec<AgentSkill>` 请求序（requested 已去重，push 即 Map.set）；
  +测试 `resolve_skills_preserves_request_and_disabled_order`；
- `skills/production_bindings.rs`：`TURN_SKILLS` 容器 HashMap→
  `Arc<Mutex<Vec<ActivatedSkillGuidance>>>`；`scope_turn_skills(initial)` 增
  轮起点预置参数；`activate_turn_skill` 已有 id 原位替换保持首插位（TS
  Map.set 语义）；`turn_skill_activations` 克隆输出；测试扩展
  （预置可见+同 id 原位更新+新 id 追加+首插序锁定）；
- `server/agent_route.rs`：`turn_seed`——requested_skills 为空（自由文本路径）
  →空预置即对齐 TS（此时 usedSkills 必空）；非空则现场解析并以
  usedSkills（resources 空）预置回合作用域；
- `interaction/skill_tool.rs`：测试调用点适配 + 534 号注释撞号清偿（3 处）。

## 门禁

- engine-rs `cargo test --all-targets`：40 目标 **1807 用例全绿**（含新增）；
- `pnpm clippy:gate`：双 crate 0 告警；
- `INKOS_DUEL=1` duel 真跑：**10/10**；
- `node scripts/node-fallback-smoke.mjs`：绿；`engine-contract-diff.mjs`：
  **41 端点 0 分歧**（活体引擎驮着本批改动跑通全契约）；
- `pnpm verify:engine-bindings`：**186 导出不变**（容器内部形态变更，零类型面）；
- 当前 HEAD（含并行 535）studio：typecheck 净 + **117 文件 894 用例绿**；
- bench:gate 豁免备案：改动面=聊天路由 skill 解析与回合集容器，不涉 bench
  覆盖热路径（写作链/检索），沿 391 号先例。

## 533 号复验补记（上轮披露缺口清偿）

浏览器自动化输入管道恢复后重试成功：点击侧栏「市场雷达」→ URL 变
`#/radar`（533 修复主体）；`reload()` 后仍停留 `#/radar` 且页面完整渲染
（面包屑「首页/市场雷达」+扫描历史，截图留证）。533 号记录已附补记。

## 教训

1. **容器选型即契约**：TS Map/Set 有序、Rust HashMap/HashSet 无序——凡跨端
   镜像的集合，序语义必须显式对齐（HashMap 换 Vec/IndexMap），否则多元素
   场景既是双端分歧又是自身非确定性；断言含序时 HashMap 版测试=掷硬币；
2. **门禁只对当时 HEAD 负责**：全绿之后并行入库的三笔复审仍抓出 3 缺陷——
   会话收尾前应对「我验证之后新落的提交」再做一轮 diff 审查；
3. **撞号三连**（533/534、535/536，同日两个并行会话）：开编号前双查目录+
   git log 不够，**写档前最后一刻再查一次**（本次开档检查时 535 尚空，
   写档时已被 ff9c86e7 占用，当即改号 536）。
