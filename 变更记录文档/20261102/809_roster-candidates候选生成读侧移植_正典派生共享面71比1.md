# 809 号：roster-candidates 候选生成读侧移植——正典派生，共享面分歧 71/1

日期：2026-11-02。前置：808 号收口登记（roster-candidates `/candidates` 首叉：node `[]`/currentChapter 0 vs rust `["苏檀"]`+cards new-entity/currentChapter 3）。

## 一、定性（与 808 同型）

- 双侧处理器**同形**：rust `ops_routes::get_roster_candidates` 与 TS 均读 `books/<id>/story/memory.db` 的 summaries（characters 列）→ `extract_character_candidates` → 对照 `entity_roster.md` → `resolve_roster_candidates` 出卡；账本缺席即返回 `{cards [], candidates [], rosterExists false, currentChapter 0}` 死空壳。
- TS 2.0 无 MemoryDB 写链（718 号定性），node 原生书账本永远缺席 → 空壳；rust 写链落 summaries 入 memory.db。
- 候选抽取/出卡函数族 716/717 已双侧同源——**全部可派生，无协议残差**（与 808 的数值分缺口不同）。

## 二、修复

`packages/studio/src/api/server.ts` roster-candidates：公共尾段抽为 `resolve(summaries, rosterPath)`；账本在位→summaries 与名册走 books/ 树（不变）；账本缺席→**正典派生**：summaries=`state/chapter_summaries.json` 行（798/808 同源，取 chapter+characters），名册=works 树 `story/entity_roster.md`（716 正典读面）；currentChapter=max(chapter)+1（rust 同式），无行→1。

测试：`roster-candidates-canonical.test.ts` 三场景——名册在位（苏檀已登记非 new-entity、林动 new-entity、rosterExists true、currentChapter 3）/ 名册缺席（全 new-entity）/ 无账本无状态（活壳 currentChapter 1）。typecheck 0。

## 三、活体

diff 报告模式：**roster-candidates ✓（200）完全收敛**；共享面 **71 对比 / 1 分歧**（仅余 quality-trend `/trend/points` 长度 2 vs 1——808 登记的执行深度差 + 数值分协议残差）。

## 四、验证

cargo:testgate engine-rs 47 目标 1970 + src-tauri 25 目标 584 + RustSec 双 0；clippy 双 0；gate:ts 八步全绿（含 engine-contract-diff + legacy 11/11）；权威 pnpm -r test core 178/983 + studio **136/722**（+1 文件 +3 测）+ cli 23/81。改动面：server.ts + 新测试。

## 五、登记（695 工项余项）

- quality-trend 残差三件（808）：TS resync 审导入章（执行深度差）/ mock 记录工具支 / 数值分协议扩展（上游面）。
- mock 撤回支（805）；TS 计数口径定性（807）；rust 重建去重（807）；UI superseded 徽标（805）。
- 695 迁移主工程共享面活体：71 对比 / 1 分歧——余项均为已定性的协议/深度/夹具级残差，无未定性分歧。

## 六、教训

1. **同型分歧用同型解法批量清偿**：798（promises）→808（quality-trend）→809（roster-candidates）三轮同一模式——"账本是投影，state 是真相；账本缺席走正典派生"；识别出模式后每轮成本递减。
2. 读侧移植前先核对**派生所需的全部函数族是否已双侧同源**（716/717 已移植）——同源则零残差，否则（808）先定协议边界再动手。
