# 1586 ai_tells 写后链基准入闸 + EN 小写单源化轮（2026-11-20）

## 逐功能靶面：写后审计链最后一块无观测热路径

1584/1585 清偿 detect_shape 后，逐功能扫写后校验链（merged_audit 每章必经）发现 `analyze_ai_tells` **无 bench 覆盖**（685 号基准面 21+ 场景不含它）。审计实现形态：dim20 段落等长/dim21 套话密度/dim22 公式化转折/dim23 列表式结构四 dim——**EN 路径 count_word 每词一次整文 to_lowercase 分配**（dim21 7 词+dim22 6 词=13 次 3KB 级文本分配+unicode 折叠；720 号 0GC 免了 regex 编译漏了这层；Zh 路径 match_indices 零分配无此放大）。

## 基准入闸 + 0GC 优化（685 号纪律：先基准取 before 后改码）

hot_paths.rs 新增 `ai_tells` 组（scan_3000zh/scan_en 双语基准，EN 夹具 ~3KB 含 hedge/transition 命中形态）：
- **before**：scan_3000zh 6.97µs / scan_en **53.6µs**（EN 每字节成本 ~12× 于 Zh——lowercase 放大实锤）
- **优化**：is_english 时整文 to_lowercase **单源化一次**（`lowered` 局部绑定），dim21/22 的 count_word 调用统一传 lowered 源+`case_insensitive=false`（词表纯 ASCII 小写=720 号注释锚定，lowered×原文 ≡ lower(content)×lower(word) 语义恒等）；Zh 路径零变化
- **after**：scan_en **47.1µs（-12.6% p=0.00）**/scan_3000zh 6.81µs（-3.5% 噪声带，Zh 无实质变化符合预期）；ai_tells 8/8 单测绿（行为锁定面）

## 边际备案

scan_en 剩余 47µs 成本=dim23 句子切分句级 String 分配（~72 句，约 1-2µs 级）+regex/子串扫描必要迭代——**收益递减线以下备案不动**。bench 热点盘点（1585 轮）：高价值 0GC 面历轮收敛完毕，本轮 ai_tells 为最后一块无观测热路径补齐。

## 基线与门禁

- bench:gate --update 全量刷新（+2 新键 scan_3000zh 6.78µs/scan_en 47.13µs 入库，旧键带内 +0.1%）+ 刷新后复跑 ✓
- cargo:testgate 双 crate **2731 passed**（2131+600）；clippy/fmt 双 0；gate:ts 十腿全绿；U+FFFD 净。

## 后续

- 下号自 1587 起；用户侧待办不变（Fork 推送 955-1586 + 打包重烤+wasmtime 50 后 e2e_startup 插件面复核）。
