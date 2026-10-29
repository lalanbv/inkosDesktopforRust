# 761 号：TS 腿原子写持久性对齐（durable-fs + 事务 staged fsync）——757 路线②

> 目标线：自主持续开发（安全可靠优先，双栈行为一致性）。760 号给 Rust 腿
> 加了 sync_all 落盘后 rename + 父目录 fsync；本批把同语义镜像到 TS 腿
> （Node 回退引擎与 studio server 直写面）。

## 一、巡检定性（757 号路线②的交付物）

| 写面 | 760 前实态 | 定性 |
|------|-----------|------|
| `atomic-write.ts` `writeTextAtomic`（studio server 7 站点 + core 导出） | `fs.writeFile(tmp)` 无 fsync → rename 重试 | 与 Rust 腿 760 前同型缺口：掉电重放可产零长/半截目标 |
| `atomic-file-set.ts` `commitFileSet` staged 写 | `writeFile` 无 fsync；**journal 已 `handle.sync()`** | 真不一致：恢复机制（journal）持久而数据不持久——掉电后 journal 判定正确却可能指向零长文件，恢复救不回数据 |
| `atomic-file-set.ts` `writeJournal` | 内容 sync ✓，rename 项未落盘 | journal.json 的 rename 可在掉电重放中丢失→误判「无事务」 |
| `logger.ts` 轮转（writeFileSync+renameSync） | 无 fsync | **豁免**：日志为可丢诊断流，轮转频率高，fsync 纯损耗 |

调用面：writeTextAtomic 全为 API 路由状态写（book.json/草稿/markdown），
用户操作级频率；与 Rust 腿同分类，fsync 成本可忽略。

## 二、变更内容（3 文件）

1. **`utils/durable-fs.ts`（新）**：`writeFileDurable`（句柄写入→sync→close，
   异常路径保证 close）+ `syncDirBestEffort`（尽力目录 fsync，Windows/
   不支持文件系统静默）——镜像 Rust 760 号两私有助手。
2. **`atomic-write.ts`**：tmp 改走 `writeFileDurable`，rename 成功后
   `syncDirBestEffort(dir)`；golden 契约（重试决策纯函数/常量）零触碰。
3. **`atomic-file-set.ts`**：staged 写改 `writeFileDurable`；committed 后对
   触碰目标父目录去重 fsync；`writeJournal` rename 后补事务目录 fsync。
   **两助手就地内联而非从 durable-fs 导入**——本文件须保持仅依赖内建/
   裸模块（见教训）；durable-fs.ts 供 atomic-write.ts 单点复用。

## 三、教训：strip-types 是隐形模块图契约

`atomic-file-set.test.ts` 的「杀死写者恢复」测试用
`node --experimental-strip-types -e "import …/atomic-file-set.ts"` 直接
加载本文件——Node 原生 TS **不映射相对 `.js` 导入到 `.ts`**，新增
`import … from "./durable-fs.js"` 即 ERR_MODULE_NOT_FOUND（vitest 内能过，
子进程红）。该文件此后新增跨文件依赖前须先检查此约束（已注释在助手处）。
`tsc` 侧 `allowImportingTsExtensions` 需 noEmit，与本仓 declaration emit
构建冲突，`.ts` 后缀导入不可行。

## 四、验证

- golden-atomic-write + atomic-file-set：9/9（含 strip-types 杀死写者子进程
  恢复测试恢复绿）；新增空内容 durable 路径边界测试。
- core 包全测：175 文件 **951 passed**（首轮根目录裸跑 vitest 扫到
  `dist/__tests__` 陈旧产物 39 文件假红——调用方式噪音，非回归；包内正式
  入口全绿）。
- gate:ts 八步全绿（test 112.5s，含 node-fallback-smoke 真实回退腿；
  契约差分 0 分歧）。
