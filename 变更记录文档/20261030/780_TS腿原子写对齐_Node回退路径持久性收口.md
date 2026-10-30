# 780 号：TS 腿原子写对齐——Node 回退路径持久性收口（757 号演进路线 #2 清偿）

日期：2026-10-30。前置：757 号演进路线五条之「TS 腿 Node 回退路径持久性对齐巡检」、779 号（Rust 腿 94 站终局收口）、761 号（durable-fs 助手已就位）。

## 一、背景与普查

779 号收口 Rust 腿后，按双栈纪律对 TS 腿（packages/core=Node 回退执行路径，活路径）做同构普查：`writeFile/writeFileSync` 调用面 **52 生产站/24 文件**，形态与 Rust 腿 779 前几乎一一镜像（同名文件同名债务：state/manager、consolidator、edit-controller、run-store、runtime-writer、book-references、logger）。

关键镜像债点名：
- `saveProjectConfig`（inkos.json——221 号「修一处漏一处」的 TS 侧同族）与 `saveBookConfigAt`（book.json「199 号=损坏即整本书不可加载」的 TS 镜像）均裸写；
- `run-store` manifest/chapter/glossary 三站裸写=142 号「同文件双轨」（save_translation_progress 走事务集、三单写裸写）的 TS 原型；
- `writeIfMissing`（stat+writeFile）=779 号「首写撕裂即永久静默损坏」的 TS 镜像；
- runner import replay 真相种子/方向文件、play-store 检查点/变体、forecast 四站、edit-controller 章节编辑三站——覆盖类与用户数据类与 Rust 同构。

## 二、处置（52 站对账闭合）

- **46 站单文件原子转换**：换装 `writeTextAtomic`（content 签名放宽收 `Uint8Array`，镜像 Rust 779 `write_file_atomic_bytes`——`writeFileDurable` 本就双型，转换零基建新增）。含 bytes 站：epub `genEpub()` Buffer、export-artifact payload、node-image buffer、short-fiction `writeBinary`。
- **1 处双件事务集**：runtime-writer context+trace 双件（compose→write 交接组）`Promise.all` 裸写升级 `commitAtomicFileSet`（镜像 Rust 779 三件套同款升级）。
- **2 处手搓 tmp+rename 收敛**：book-references `writeManifestAtomic`（uuid tmp 无 fsync/重试→writeTextAtomic）、logger `rotateIfOversized`（固定 tmp 名并发互踩→pid+时间戳唯一名+`fsyncSync` 落盘后 rename，sync 上下文内联三行）。
- **2 站 staging 边界销账**：runner.ts 建书 staging 内 brief.md/current_focus（staging 经 set-commit 收口后整体就位，单文件原子化是冗余）——与 779 号「staging+rename 边界是豁免非漏网」判定一致。

## 三、验证（全绿）

- tsc --noEmit 归零；core vitest 全套 **175 文件 960 passed**（含 golden-atomic-write 新增 2 测：bytes 全量替换无 tmp 残留 + 空 bytes 落盘）。
- gate:ts 八步全绿（build/typecheck/test/audit:npm/rust-bin/node-fallback-smoke/engine-contract-diff/export-epub-smoke），engine-contract-diff **0 分歧**——写机制硬化不触任何契约面，实证。
- Rust 零触碰（rust-bin 新鲜度闸过）；转换后复扫：生产面裸写残留恰为 2 个 staging 销账站。
- Rust 侧原子写金样（golden_atomic_write_diff.rs）读同一 vectors 文件未动，双栈契约常量/重试决策不受影响。

## 四、教训

1. **双栈债务是镜像的**——TS 腿与 Rust 腿同名文件同名债务（同源移植），一侧清偿后另一侧必须做镜像巡检，否则「Rust 已安全」给人全仓安全的错觉（Node 回退路径仍在用裸写）。
2. **TS 批量转换的 import 管理比 Rust 难**——多行 import 的「最后 import 行」启发式会插进开括号后（两文件破损即时被 tsc 抓住）；批量文本变换后 tsc 是唯一可靠的验收线。
3. **签名放宽优于新增函数**——TS 联合类型习惯下 `writeTextAtomic(path, string | Uint8Array)` 一处放宽即覆盖字节站（Rust 无联合故需独立 `write_file_atomic_bytes`），双栈语义仍逐字对齐。
