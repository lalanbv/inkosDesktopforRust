# 884 号 · 工程轮：work 域 M1b——端点层 + adopt 最小环 + 交替读写互证全环

> 881 设计 §五 M1b 落地。上游新增 works 域（695 记账最大未迁域）的 L1 读写面在
> Rust 引擎接线：GET 全域 + adopt + skills documents；契约差分 works 域从「声明
> 确认」升级为「活体对比」；双引擎交替读写互证 18/18 全环通过（881 验收三件套
> 之三：WorkInspector/works 区/版本抽屉/adopt 在 Rust 引擎下活体的数据面前提）。

## 一、M1b 交付面（engine-rs）

### 端点层（server/work_routes.rs，8 条 + skills documents）

| 端点 | 形态 | 错误面（逐端点对齐 TS，勿"修复"） |
|---|---|---|
| GET /works?profileId= | `{works:[{...work, artifactCount}]}` updatedAt 降序 | 无局部 catch → 500 INTERNAL_ERROR |
| GET /works/:id | `{work, episodes(limit 50)}` | 局部 catch：ENOENT→404 / 其余→400，`{error:"<msg>"}` 纯字符串 |
| GET /works/:id/artifacts | `{workId, artifacts}` | 同上 |
| GET .../revisions/:rid?download=1 | content（text/json）/ dataUrl（binary）/ 附件流（attachmentDisposition 移植） | checksum 不符→**400**（TS ApiError(409) 被局部 catch 降级的同款行为）；快照缺→404 |
| POST .../adopt | `{action: ActionResult}`（episode 最小环） | 无局部 catch → 工具错误全走 500 INTERNAL_ERROR 固定文案（TS onError 同款） |
| GET /episodes?workId&profileId&status&limit | `{episodes}` | 局部 catch 400 |
| GET /episodes/:id | `{episode, events}` | **无局部 catch：未知 episode = 500 固定文案**（TS requireEpisode 抛普通 Error 落全局 handler，勿"修复"为 404） |
| GET /profiles | 内置 9 + `.inkos/profiles/*.json` replace 归并，id 升序 | 无局部 catch → 500 |
| GET /skills/:skillId/documents | `.md`/`.txt` 递归收集，2MB 上限→413，符号链接跳过 | 404 SKILL_NOT_FOUND / 400 INVALID_SKILL_ID / 413 typed |

**双端对齐变更（TS 侧一行）**：skills documents 双端按 path 排序（原 readdir 顺序
对编辑器 UI 无语义；排序使契约确定性=差分活体 body 对比前提）。`safeChildPath`
移植（词法 resolve+收容，`rel===""` 允许的 TS 同款）。

### 新 harness 模块（移植自 packages/core/src/harness/*）

- `profile_registry.rs`：内置 9 profile 静态表（builtin-profiles.ts 逐行对照，含
  contextRecipe{task,skills,work}/artifactSchemas/confirmation 默认）+ 自定义归并
  （version literal(2) 检查补齐——contracts.validate 未含字面量）。
- `artifact_edit_policy.rs`：artifactDomainOwner + assertGenericArtifactEditable
  忠实移植（short-fiction replacement 豁免、film bundle 投影归属、recovery 载荷）。
- `artifact_validation.rs`：validatedArtifactWrites——TS **失败**分支逐字对齐
  （exports DERIVED 指回源工件 / drafts DERIVED 带章节号 recovery / final 派生 .md
  DERIVED / packaging .md DERIVED / JSON parse ARTIFACT_INVALID）；**成功且需 L2
  投影再生的分支 fail-closed**（`ARTIFACT_DOMAIN_PROJECTION_UNAVAILABLE`：短篇
  authority 再生/销售包渲染/翻译所有权/story-graph 投影=未迁域，裸写会造出「Node
  预期投影已再生而实际没有」的腐化清单态，违反交替读写硬门禁——M3 移植对应
  runner 时移除）。
- `source_sync.rs`：syncWorkSourceArtifacts 全移植（350 行语义密）：源目录全量
  重扫描∪pending 键、rev-<checksum16> 幂等复用、快照补齐（legacy 就地回填/前值
  取证冲突）、accept 面翻转+removedAt/removedPath 淘汰检测、kind 归一
  （storage-path-v1 门控+legacy 搬迁）、原子提交（work.json 结构体序列化保字段
  声明序）、.db/.sqlite=rusqlite backup 快照读（Cargo.toml 增 backup 特性）。
- `artifact_reader.rs`：readArtifactRevision（realpath 收容+checksum 强校验）。
- `adopt.rs`：executeExplicitCapabilityTool 的 adopt 面等价——episode 环
  （recover→create→episode-started(baseline)→action-started→work-artifacts-synced
  (evidence)→action-completed→episode-completed；失败=action-failed(facts 白名单
  code/recovery)→episode-failed 吞二次错）+ per-work tokio Mutex（write_next
  惯例）+ normalizeToolResult 的 artifact refs 差异面（before/after
  currentRevisionId）。`runWithAgentTrajectory` 确认纯 ALS 包裹（881 判定），Rust
  直呼工具体。adopt 请求体 `Option<Option<String>>` double-option（TS 严格 `!==`
  下缺字段≠null≠字符串——漏 rename 曾致 500，测试揪出）。

## 二、交互缺陷修复（本轮揪出，非本轮引入）

1. **episode_store 自死锁（M1a 备案「WAL 竞态待查」真根因定案）**：`finish()`/
   `bind_work()` Ok 臂持 std Mutex 调 `require_episode()` → 不可重入自锁。M1a
   的 #[ignore] episode 用例即挂在此。修复=锁作用域收紧（drop 后取行），
   **#[ignore] 解除**（879 号备案清偿；"WAL 竞态"定性推翻——非 WAL 问题）。
2. **list_episodes 编号参数隐患（M1a 隐患，882 验收漏网）**：SQL 用 `?1..?4`
   编号参数（重复引用不增计数），M1a 按出现次数传 11 参 → rusqlite 拒绝；因
   唯一调用方被 #[ignore] 遮蔽从未真跑。修复=传 4 参。教训：**#[ignore] 是
   隐患掩体**——被忽略用例的路径=无测试路径。
3. adopt 失败面事件：action-failed 载荷 facts 白名单（code/recovery，path/
   capabilityId 挂错误对象但不入 facts——TS actionFailureFacts 同款）。

## 三、测试（四线全绿）

- **Rust 端点活体** `tests/work_domain_m1b.rs`（11 用例）：works/episodes/
  profiles 形状+过滤+排序；revision content/download/checksum-400/快照缺 404；
  adopt 全环（事件序列逐项断言+清单翻转+快照落位）；失败三态
  （未知修订/expected 漂移/非 source 派生=独立 episode 行+事件码归类）；fail-
  closed 边界（authority 路径合法 JSON→PROJECTION_UNAVAILABLE，非法 JSON→
  ARTIFACT_INVALID=TS 先 parse 后校验的同序）；变异三类（checksum 篡改红/
  currentRevisionId 漂移红/strict 未知键红）；source_sync 确定性
  （artifactIdFor CJK/清洗金标准+新文件 accept+淘汰检测 removedPath）。
- **金标准锁双端落盘**：`packages/core/src/__tests__/work-store-cross-engine.test.ts`
  **本轮新建（882 备案缺口清偿——882 只落了 Rust 腿，mod.rs 却写「逐字共用」）**
  6 用例：work.json parse→再序列化逐字一致/strict 双拒/内置 profile 金标准
  （含 contextRecipe 全字段，`JSON.stringify` 字节级）/createWorkManifest+排序/
  episode 行形状/**rev2 checksum 自锚定**（防金标准自身漂移）。Rust 侧
  `golden_builtin_profiles_match_ts_shared_strings` 共用同两字符串（语义全等+
  顶层字段序由结构体声明序保证；嵌套 map 键序无 JSON 语义，备案）。
- **契约差分升级**（engine-contract-diff.mjs 报告模式）：works 域 8 端点活体
  归一化深比对（episode 随机 id→$EPISODE、事件时间→$TIME；**夹具不对称备案**：
  node 腿独跑 migrateLegacyWorks → /works 全集多 legacy 迁移工作，对比锚定对称
  夹具 walk-diff 子集）。首轮用 `firstId` 取到 node-only 工作致深链 404 分歧——
  教训：活体对比的锚 id 必须取**双腿对称夹具**，非集合首项。末轮 **8/8 一致
  0 分歧**；works 深链从「+2 静态声明」转正为活体。
- **交替读写互证** `scripts/work-domain-alternation.mjs`（881 §四.3 兑现）：
  同一根 Node 写→Rust 读→Rust adopt→TS zod strict 读（876 号活体闭环）→Node
  引擎再读，**18/18 全环通过**。须 preseed inkos.json（引擎启动门槛，复用差分
  脚本 preseed 形态）；works 域零 LLM 依赖（LLM 指死端口，无 mock）。
- **门禁汇总**：cargo test 全量 1803 绿（含解除 ignore 的 M1a episode 用例）；
  cargo:testgate 2008+584 双 crate 全绿；clippy:gate 双 0；gate:ts 八步全绿
  （含 engine-contract-diff 步骤=works 活体 8/8 进门禁默认链）。

## 四、备案

1. **共享面 1 分歧（既有，非本轮）**：`/books/:id/quality-trend /trend/points/1/
   issueCount` 两轮差分均在——与本轮改动面零交集，待后续轮定性（数据型指标，
   疑 fixture 噪声或既有漂移）。
2. **adopt 迁移边界（fail-closed 域）**：短篇 authority/销售包/翻译
  manifest·glossary·章节结构/story-graph 的**成功分支**在 Rust 显式报
  ARTIFACT_DOMAIN_PROJECTION_UNAVAILABLE（500 固定文案面）。TS 侧这些路径成功
  并再生投影——Rust 静默裸写=腐化态，显式失败=可行动边界。M3 移植 runner 时
   接通并移除本边界（测试 adopt_fail_closed 已锚定该边界行为）。
3. **M2 待办**（写面）：profiles POST/PUT、skills documents PUT、work 创建端点。
4. walkthrough-fixture 的 Rust 腿 /works 断言现为 200（原「只提示不失败」的
   404 容忍已无必要，容后随 885 收敛）。

## 五、验证清单

cargo test 1803 绿 / testgate 2008+584 / clippy 双 0 / gate:ts 八步
/ 差分 works 活体 8/8 / 交替互证 18/18 / TS 金标准 6/6 / M1b 11 用例 / U+FFFD 扫描干净
