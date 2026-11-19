# 1600 空 object-map 双端 schema 漂移修复轮（2026-11-20）

## 抓虫：同款 inkos.json 双端分裂——Rust 绿 / Node 拒启（回退腿失效）

Node 腿对称补走查起服务时浮出：`llm.services: {}`（空 object-map）在 **Rust 腿接受**（serde HashMap，1595/1599 走查背书）、**Node 腿 studio server ZodError 拒启**（`Expected array, received object`，exit 1）——1596 未暴露是因当时 cwd=engine 无 inkos.json→auto-init 走 TS 自家合法形态；手写夹具命中漂移。影响面：**桌面回退腿对 services-object 形态的项目拒启=回退失效**（709/1429 号「Rust 写的形态 TS strict 拒」同族第三例）。

## 根因与修复（packages/core/src/utils/effective-llm-config.ts:89）

688 号已建 `normalizeServiceEntries` 双形态兼容（object-map→数组展平），但回写条件 `services.length > 0` 使**空 map 归一出的空数组不回写**——原始 `{}` 留守 `config.llm.services`，被 `ProjectConfigSchema.parse`（strict array）拒。完全解释三方形态：非空 map（duel 夹具）兼容生效绿/空 `{}` 炸/Rust 绿。修复=回写条件改「原始值在场即回写」（`llm.services !== undefined`）——undefined 仍不回写（缺省），非空 map 与原生 array 形态行为不变。

## 验证链

1. **core 单测 23/23**（+2 新用例：空 map 修复锁「不残留 object 形态/业务错误允许 ZodError 不允许」+非空 map 688 行为补锚定「custom: 键回填 name/service」——用例 2 首版夹具带 `model` 键被 entry strict schema 拒，按 duel 夹具真形态勘误）。
2. **core 全量 1078 passed**（1592 基线 1076+2，无面外回归）。
3. **dist 重烤+Node 腿活体**：修复前同夹具 ZodError exit 1 拒启→修复后（重烤 core dist）服务启动+SPA 200+books API 正常响应。
4. **gate:ts 十腿全绿**（build/typecheck/test 111.7s/audit:npm/rust-bin/node-fallback-smoke 35.9s/engine-contract-diff 36.6s/export-epub-smoke/work-alternation/check-routes——smoke 与差分腿吃修复后 dist=dist 链活体）。
5. **duel 真跑回归**：10 枚=1 真跑绿（bin_process_write_next_llm_resolution：Node bin 进程吃修复后代码读 inkos.json 服务项 3 次触达+env 零触达）+9 枚 688 豁免响亮记账（契约源守卫设计形态：TS 腿=上游 2.0，迁移落地自动复活）——0.97s vs 不设 env 0.00s 对照确认非早退。

## 形态定案（备案非 BUG）

books 列表双端差异：Node `listBooks()`=listWorkManifests 扫 **2.0 works 树**；Rust=扫 books/<id>/book.json（产品引擎树）——688 号定线的双栈分离形态（TS=2.0 基底/Rust=产品引擎），非本轮漂移。

## 清场

端口释放+无孤儿+tmp 清理+U+FFFD 净（TS 源中文注释）。

## 后续

- 下号自 1601 起；用户侧待办不变：① Fork 图形端推送（955-1600）② 真实 Tauri build 打包+安装包验证。
