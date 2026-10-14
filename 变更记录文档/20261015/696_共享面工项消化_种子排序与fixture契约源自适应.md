# 696 号：contract-diff 共享面工项消化（种子排序双端对齐 + fixture 契约源自适应 + work 迁移边界定位）

日期：2026-10-15。类型：fix(engine-rs)+fix(scripts)。前置：695 号（7dd30073）。

## 一、承接（695 号共享面 6 分歧工项）

695 号三域报告产出的共享面 6 分歧逐项处置，**分歧 6 → 2**，且剩余 2 项定位到同一根因（2.0 work 迁移边界）。

## 二、修复一：Rust 种子兜底排序缺口（engine-rs 真缺陷，活体捕获）

- **根因**：TS store `listAssets` 种子兜底 = `sortAssets([...seedsFor(kind)])`（id 序，mystery 最先），而 engine-rs `list_assets` 种子兜底返回 `seeds_for(kind)` **源码序**（urban→xuanhuan→mystery）——`sort_assets` 在 merge/import 路径都有、唯独种子兜底漏了。双端种子**数据**逐字段一致（含 mystery_investigation 全文），分歧纯属顺序。
- **修复**：`list_assets` 种子兜底路径排序（`sorted_seeds` 闭包，三处返回点统一），**不影响 `genre_base_seeds()` 源码序**（既有测试 `genre_base_seeds().remove(0)` 等按源码序断言，实跑确认不受扰）。
- 695 报告的另一个发现随之修正：/skills 分歧是**键序假分歧**（裸 stringify 对比双端序列化序），报告比较改用与严格比对同口径的 `prune` 规范化（键排序+VOLATILE 剔除）后 /skills 转绿——skills 正文内容本就一致。

## 三、修复二：fixture 契约源自适应（`writing` 段 2.0 化）

- **根因**：walkthrough-fixture 的 inkos.json 带顶层 `writing.reviewRetries`（fork 写作评审循环参数，engine-rs 契约）——上游 2.0 `ProjectConfigSchema` strict 无此键，TS 腿 resync 链 400 INVALID_PROJECT_CONFIG。
- **修复**：fixture 探测 `/api/v1/prompt-packs`（同三域守卫判据）区分契约源——fork 引擎写入 `writing` 段；2.0 引擎省略（2.0 无评审重试旋钮，语义即上游固定行为）。修复后 TS 腿 resync 越过 config 段。

## 四、新发现：2.0 work 迁移边界（剩余 2 分歧的同一根因）

TS 腿 resync 越过 config 段后新断点=`works/镜花水月/work.json` ENOENT——**2.0 的 resync 是 work-based**（操作 works/<id>/work.json），legacy 书需先走 `inkos work migrate`。/books 0 书与 /doctor bookCount 分歧同根因（TS 侧 legacy 书未迁移注册）。定性：**迁移边界工项而非缺陷**——fixture 对 2.0 腿执行 work 迁移是迁移项目本体（688 §五）的一部分，本号不做强塞（把迁移逻辑塞进 fixture = 在测试夹具里实现迁移，方向错误）。

## 五、度量与门禁

- contract-diff 三域报告：**共享面 8 对比 / 2 分歧**（6→2：asset-library×3 + skills 消化；剩余 /books + /doctor = work 迁移边界）；fork-only 52/52；上游新增 9/9+2 深链声明；exit 0。
- cargo testgate（INKOS_DUEL=1 真跑）：见门禁补录（engine-rs list_assets 排序变更全量回归）。
- studio/core 零改动（fixture 脚本与 engine-rs 之外无 TS 变更），contract-diff 为唯一 TS 侧改动面。

## 六、教训

1. **排序是契约不是实现细节**：TS store 排序、Rust 源码序——「数据一致但顺序不同」在活体差分里就是分歧；双端镜像的函数清单要逐路径核对（Rust 的 sort_assets 在 merge/import 都在、唯独种子兜底缺席，正是「逐路径」漏项）。
2. **裸 stringify 不是契约比较**：键序差异会让真分歧（数据）与假分歧（序列化序）混在一个数字里——规范化口径必须与严格比对同源（prune），否则度量本身不可信。
3. **fixture 修复的止盈点**：fixture 能修的（config 形态）修掉，触及迁移本体（work 迁移）就停手记录——夹具里实现迁移逻辑 = 方向错误，工项该回到迁移项目里。
