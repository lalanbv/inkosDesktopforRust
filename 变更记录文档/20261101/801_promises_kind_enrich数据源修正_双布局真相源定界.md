# 801 号：promises kind enrich 数据源修正——双布局真相源定界

日期：2026-11-01。前置：800 号字段级 worklist（node 行缺 kind 键 + openedAt/lastAdvanced 覆写疑云）。

## 一、「node 行缺 kind 键」清偿（数据源错位修正）

**根因**：promises 处理器的 kind enrich 读 `works/<id>/source/story/pending_hooks.md`——该文件是 **renderHooksProjection 的渲染投影（7 列，无分类列）**（写链 saveRuntimeStateSnapshot 从 state 渲染落盘），`parsePendingHooks` 的 kindIdx=-1 → kind 全 undefined → enrich no-op。rust get_promises 同源读 **books/<id>/story/pending_hooks.md=原始 14 列台账（含分类）**——数据源本身不同。

**修复**：node 处理器 kind enrich 源切换 `books/<id>/story/pending_hooks.md`（原始台账，rust 对偶同源），books 副本缺失（node 原生书）降级 works 树（无分类列→无 kind，优雅缺省）。

**活体**：/tmp/wtestA promises timeline kind 全命中——H04 worldview / H01 suspense / H02 emotion / H03 artifact（与 rust 逐项一致）。

## 二、双布局真相源定界（本轮定性成果）

| 文件 | books/<id>/（迁移源/原始台账） | works/<id>/source/（正典树） |
|---|---|---|
| pending_hooks.md | **14 列原始台账（含分类/kind）** | 7 列渲染投影（无分类） |
| 用途 | kind enrich 数据源（rust 对偶同源） | 正典状态读面（state four-piece） |

写链落盘规则：settle 应用 → state/*.json 权威 → 渲染投影覆盖 works 树 md（分类列信息在渲染中丢失=形态使然非 bug）→ **kind 类 enrich 必须回 books/ 原始台账**。

## 三、openedAt/lastAdvanced 覆写疑云（登记待插桩）

活体实证：node state hooks.json **全部 hook startChapter=1**（=sync settle 的 delta.chapter）——upsert 携带的 startChapter（3/2）被覆写。疑点=write-next 管线的落盘后同步阶段（resyncChapterStateAndAudit，delta.chapter=1）以 baseline snapshot 0（空台账）重建 hooks 时 startChapter 注入 delta.chapter——**需 --keep + 服务端插桩定位确切覆写点**（静态分析无法定界）。登记为独立插桩轮。

## 四、验证

- 活体：promises timeline kind 全命中（worldview/suspense/emotion/artifact 与 rust 一致）。
- core 979/979 + 权威 pnpm -r test 三包全绿 + tsc 归零 + clippy 双 0 + testgate 1956+584 零漂移 + gate:ts 八步全绿。
- rust 零触碰（纯 studio 处理器修复）。

## 五、教训

1. **双布局的「同名文件」是两个数据源**：books/<id>/story/pending_hooks.md（原始 14 列台账）与 works/<id>/source/story/pending_hooks.md（7 列渲染投影）——同名不同内容，enrich 类读取必须选对真源（697 号迁移边界「正典读面走 works 树」的例外：分类列只在原始台账）。
2. **渲染投影是有损形态**：写链从 state 渲染 md 落盘时，state 无的键（kind）渲染自然缺列——「读回渲染物补 state 缺失字段」方向必然失败，enrich 的数据源只能是未渲染的原始台账。
