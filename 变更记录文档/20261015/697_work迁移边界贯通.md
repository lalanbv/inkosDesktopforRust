# 697 号：work 迁移边界贯通（fixture 迁移接入 + 共享面收敛至字段级语义）

日期：2026-10-15。类型：fix(scripts)+test(studio)。前置：696 号（1058e0c7）。

## 一、承接（696 号定位的 work 迁移边界工项）

696 号把剩余 2 分歧（/books 0书、/doctor bookCount）定位到「legacy 书需 2.0 work 迁移注册」——本号实施并贯通。

## 二、实现

- **contract-diff 共享面新测 `work-migration-books.test.ts`**：中文书名（镜花水月）legacy 播种 → `migrateLegacyWorks(root, {apply:true})` → /books 列出（id 保真）+ /doctor bookCount=1。实证 `WorkResourceIdSchema` 接受中文 id（仅拦路径不安全字符）——fork 的中文书 id 契约在 2.0 迁移链上无损。
- **fixture 迁移接入**（walkthrough-fixture 步骤 2.5）：契约源自适应同款判据（/prompt-packs 404=2.0 腿）→ `migrateLegacyWorks(root, {apply:true})`，逐条打印迁移结果；fork 腿跳过（原生列 books 目录）。
- **施工实录两断点**：①scripts/ 不在 workspace 依赖链，`import("@actalk/inkos-core")` 无法解析——改导入 core 构建产物相对路径（`packages/core/dist/index.js`，缺失时给可行动指引）；②/doctor 响应 shape 是扁平 `checks` 对象（`bookCount` 在顶层）非嵌套 `checks.bookCount`。

## 三、度量变化（contract-diff 实测）

- **/books 与 /doctor bookCount 分歧消解**：双引擎 /books 各列 1 书（迁移注册后 TS 腿可见）——696 号的「长度 0 vs 1」与「/bookCount」两处分叉消除。
- **剩余 2 分歧收敛为字段级语义**（深水项，已编目）：
  1. `/books/0/chaptersWritten`：fork 语义=持久化进度链章数（487 号，fixture 播种 1 章 → 1）；2.0 语义=自有计算（work manifest 驱动）——书籍摘要语义对齐=迁移项目深水项；
  2. `/doctor`：llmConnected/booksDir 等环境身份面——684 号既定豁免类（双端探测路径不同，产出语义等价）。
- 共享面度量：8 对比 / 2 分歧（均字段级），fork-only 52/52、上游新增 9/9+2 维持。

## 四、门禁

- studio **646/646**（645+1）、typecheck 0。
- engine-rs 零改动（696 号种子排序已随 1058e0c7 门禁双零漂移）。

## 五、教训

1. **迁移边界贯通的正解是用迁移方设计的机制**（migrateLegacyWorks），而不是在消费面（/books lister）加 legacy 兼容——前者收敛后者发散。
2. **根级脚本的 workspace 解析断点**：scripts/ 下 import workspace 包名不可解析（pnpm strict 不提升），相对导入 dist 产物是正道——且 dist 缺失时的错误信息要给可行动指引（先 build core）。
3. **响应 shape 断言先看真实 body 再写**（692 号 head 语义教训同型）：/doctor 的 bookCount 在顶层不在 checks 嵌套里。
