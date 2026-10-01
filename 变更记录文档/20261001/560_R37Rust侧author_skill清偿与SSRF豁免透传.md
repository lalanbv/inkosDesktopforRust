# 560 号：R37 Rust 侧 author_skill 清偿 + research 面出站防线豁免透传（556 号回归修复）

日期：2026-10-01
类型：feat(engine)+fix(engine)，500 号先例清偿 + 556 号回归出清

## 选题来路

559 号 R37 写面 TS 先行，Rust 侧备案清偿（498→500 先例：TS 先行、Rust 补偿）。
本轮同批清偿 556 号遗留回归：`e2e_write_next_contract::research85` 集成测试
暴露 mock tavily 返回本地 URL 被 556 号 SSRF 防线拦截（`Partial failures: 2.`
vs `none.`）——**556 号门禁只跑 `cargo test --lib`，独立 test 目标（tests/
目录）盲区**，被本轮 clippy --all-targets 触碰 test 编译时暴露。

## 实施（Rust 侧）

1. **治理面三件**（与 TS 559 号同构）：
   - `AgentSkill` 加 `disable_model_invocation: Option<bool>`
     （serde rename disableModelInvocation + skip_none——526 号 skip_serializing
     语义对齐 TS optional）；
   - `parse_agent_skill_document` 映射 frontmatter kebab-case
     `disable-model-invocation`；
   - `resolve_skills` 把 governance 技能并入拦截集（requested 强制也跳过），
     **不进 disabled_skill_ids 输出**（「显式禁用清单」语义与 TS registry.ts
     同构——TS 侧同批把 disabledSkillIds 计算移到 governance 合并前）。
2. **author_skill ToolDef**（interaction/author_skill_tool.rs）：description
   与 TS 逐字同文（差分器硬对照）；scope=Session（**Project/BookSession 是
   文件三件双作用域分层专用**——首版误选 Project 被
   project_layer_projection 测试拦截）；MutationKind=ProjectWrite；安全名/
   保留字/只增不改/全源撞名/load_available_agent_skills 回读闭环/frontmatter
   JSON 标量注入防御，全链与 TS 同构；registry 装配 material 后（声明序末位）。
3. **research 面豁免透传**（556 号回归修复，双端同构）：
   - `fetch_url(url, max_chars, allow_private_egress: bool)`——防线条件化；
   - `ResearchSearchConfig.allow_private_egress`（inkos.json
     `researchSearch.allowPrivateEgress`，缺省 false 走防线）→
     TavilyTransport 透传到 fetch；
   - TS：fetchUrl 第三参 options + ResearchSearchConfigSchema.allowPrivateEgress
     + createResearchWebTool deps.fetch 透传——**豁免是用户显式配置语义**
     （自建网关/内网知识库部署，与 search_web base_url 不做校验同一理由），
     非测试专用钩子（e2e 的 mock 配置同键开启）。
   - TS registry.ts：disabledSkillIds 计算移到 governance 合并前
     （显式禁用清单语义；governance 停用不混入）。

## 门禁（八项全绿）

engine lib 1446 + e2e_write_next_contract 206（research85 修复）+ golden
全绿；clippy:gate 双 crate 0；src-tauri 468；core tsc 0 + chat-tool-set/
author-skill/spill/egress 19 测试；gate:ts 七步（差分器交集 32 件逐件 0
分歧——author_skill 双端哈希对齐，node-only 备案 13 件）；duel 真跑 10/10。

## 教训与备案

- **`cargo test --lib` 不含 tests/ 目录独立 test 目标**（569 号「cargo check
  不覆盖 test 目标」同类教训第二例）——e2e/golden 目标的回归要显式
  `cargo test --test <name>` 或全目标跑；clippy --all-targets 只保编译不保
  行为。
- **ToolScope 三变体语义**：Project/BookSession 是文件三件双作用域分层专用
  （「回环尾面文件投影」），普通工具族挂 Session——scope 选择错误被
  project_layer_projection 名单锁当场拦截（名单锁的价值实证）。
- **gate 后跑的差分器要确认 bin 新鲜度**：cargo build 失败（cwd 错误）时
  差分器可能跑旧 bin——活体对照的引擎二进制必须与 HEAD 构建一致。
- fetch_url 签名变更=engine-rs 内唯一调用点（research_tool）同步改，
  index.ts 导出面 TS 第三参可选零破坏。
