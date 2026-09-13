# 414 · get_writing_stats 死代码清理（顺手项）

日期：2026-09-14　性质：清理批　前置：413 号对照结论

## 内容

- 删除 `ops_routes::get_writing_stats`（孤儿 handler，双 bug：tuple 序列化
  不符前端 ChapterStatRow 期望 + 读 `story/chapter-index.json` 可疑路径；
  411 号 rows 版为正确链路且前端已接）；
- `check-routes.mjs` 白名单同步移除该项。

## 门禁

check-routes：handler 形态 108 → **107**，孤儿 0，exit 0；
INKOS_DUEL=1 39 单元 1787 用例 exit 0。

## 推送状态

origin/develop=e555d6f3；剩余 **19 笔**未推（402–414，3f268833 起）——
请在 Fork 图形端推送并核验 origin/develop 追平本地 develop（414 提交）。
