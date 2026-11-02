# 854 号：双端章节文件识别恒等补锁 + audit 门禁族续修（单数行解析 / g 正则状态 / GHSA id 钉定）

日期：2026-11-03。前置：845/847/848 号。两线并行：A 线把 845 号「双端恒等」从 ASCII/BMP/小章号巧合升级为全域恒等；B 线是 846/847/848 恒绿事故族的第三轮扫尾（上两轮修的是「恒绿」，本轮修的是「恒绿修完仍残留的三处解析/放行面漏洞」）。

## 一、A 线：章节文件识别双端恒等补锁（packages/core + engine-rs）

845 号合并了双端择优裁决的**结构**（正典优先+类内文件名序），但类内序与章号解析的恒等此前只在 ASCII 文件名与小章号下靠平台巧合成立。本轮压出三处真分叉，全部补锁：

1. **类内文件名序 = 码点序**（`chapter-files.ts` 新增 `compareByCodePoint`）：JS `<` 按 UTF-16 码元序，增补平面字符（emoji 代理对 0xD800-0xDFFF）与 U+E000-U+FFFF（全角标点 `！（），：`）相对次序与码点序**相反**；Rust `&str` 字节序=码点序（UTF-8 保序）。活体分叉：同章双正典 `0001_开局！.md` / `0001_开局🔥.md`，旧实现 **TS 取 emoji 文件、Rust 取全角叹号文件——双引擎开不同章**。比较器逐码点、零中间分配（不展开数组、不转码），`preferredChapterFile` 接入。双侧测试锁双向入参。
2. **章号上界 = u32::MAX**（`MAX_CHAPTER_NUMBER`）：TS `parseInt` 对超 u32 前导号给浮点整数（`Number.isInteger(1e20) === true`）列为章，Rust `parse::<u32>` 失败判非章——双引擎索引分叉。TS 侧按同一上界拒绝，`chapterFileNumber / parseChapterFileName / selectChapterFiles / findChapterFileByNumber` 四面向同步（`4294967296_超界.md` 不列章）。
3. **数字类 = 显式 `[0-9]`**（**双侧**：TS `chapter-files.ts` 两正则 + Rust `manager::is_canonical_chapter_file` 与 `rebuild_file_re` 两处）：regex crate 的 `\d` 默认 Unicode 数字类（`\p{Nd}`，含全角 `０１２` / 阿拉伯-印度 `٠١٢`），JS `\d` 恒 ASCII——此前非 ASCII 数字前缀 Rust 判章、TS 判非章（混合形态 `1２_x.md` 双端还可能各判各的）。双侧统一显式 `[0-9]` 并互注锚定；`chapter_filename.rs` 补镜像测试（全角/阿拉伯-印度数字判非章、`1２_x.md` 双端同判章 1）。

## 二、B 线：audit 门禁族续修

1. **`audit-rust.mjs` 单数汇总行解析**（847 自检永远走不到的分支）：`vulnerabilities?` 匹配不到单数形 `error: 1 vulnerability found!`（`y` ≠ `ies`，0.22.2 活体实证）——恰 1 条漏洞时 vulns 解析为 0。门禁因 cargo-audit 非零退出仍红（不放行），但细节不打印、诊断错向。改 `vulnerabilit(?:y|ies)` 双形通吃。
2. **`g` 正则 `.test()` 有状态**：`YANK_NOISE_RE` 带 `g` 标志又被逐行 `.test()`——`g` 正则 `lastIndex` 跨调用保持，噪声行隔行漏进错误尾部（实测 [true,false,false]）。行级判断改字面量 `includes`，计数保留 `g`。
3. **单漏洞夹具 `known-vulnerable-single.Cargo.lock`**：smallvec 1.6.0 → 恰 RUSTSEC-2021-0003 一条（活体实证恰 1），专门锁**单数汇总行**解析——多漏洞夹具的 ≥1 断言永远走不到单数分支（847 号盲区同型：测试断言宽度小于事故形状）。自检另加 **parity 断言**（stderr 汇总数 = stdout RUSTSEC 条目数——部分解析=检测器退化）。夹具头明示变红时换 crate 不放宽断言。
4. **`audit-npm.mjs` 白名单按 GHSA id 钉定**：白名单从「整模块放行」收窄为「模块 → 已定性 GHSA id 集合」——模块级匹配把同模块**未来新增的任意 advisory**（哪怕 critical）静默当作已接受风险，放行面大于定性面（846/848 恒绿族同型）。活体实证：**@vitest/mocker 的 GHSA-qxg6-89mg-rqv8**（2026-10 新发，dev-server WebSocket 未认证 mock 注册代码注入）从未被单独定性、仅因模块名顺带放行。本轮补定性接受（patched >=4.1.12 同依赖 vitest 4 跨 major；暴露面同类：仅测试期 dev server 可达，须先连上本机 WS），全部九条白名单逐条钉定 id，自检加第 4 夹具锁「已白名单模块 + 未钉定新 id → 红」。

## 三、实证

| 面项 | 结果 |
|---|---|
| `packages/core` vitest 全套 | **998/998** 全绿（180 文件，含 chapter-files 17 项含新增 3 组） |
| `packages/core` tsc --noEmit | ✓ 0 错 |
| `engine-rs` cargo test --lib 全量 | **1546/1546** 全绿（含 state:: 173，含新增 u32 上界/码点序/非 ASCII 数字测试） |
| `pnpm clippy:gate` | ✓ 双 crate 0 告警 |
| `audit:rust` 全路径 | ✓ engine-rs/src-tauri 各 0 漏洞（advisory-db 当日）+ 双夹具自检 ✓（7 漏洞 parity / **1 漏洞 parity——单数行解析活体证明**） |
| `audit:npm` 全路径 | ✓ advisory 合计 10 全部 id 钉定放行 + **4 夹具自检全绿**（含新增「未钉定新 id→红」） |

## 四、杂项

`.mimosa/`（mimosa 插件会话级 hook 状态，工具溢出物，834 号同类定性）加入 `.gitignore`——不入库、不影响门禁。

## 四b、补验（修复后全电池 + 模拟真人走查，另一会话独立复跑）

- **全电池**：`gate:ts` 八步全绿（build 18.5s/typecheck 17.6s/test 135.9s/audit:npm 3.8s/rust-bin 增量/node-fallback-smoke/engine-contract-diff/export-epub-smoke）；`clippy:gate` 双 0；`cargo:testgate` engine-rs **1984**（+2=新增码点序/上界镜像测试）+ src-tauri 584，audit:rust 双 0（DB 当日）**双夹具自检 ✓**；`INKOS_DUEL=1` 经 testgate 注入真跑（日志零跳过，9 duel 测试 ok）。
- **干净锁反证**：`INKOS_AUDIT_SELFCHECK_FIXTURE=干净锁` → 自检 exit 1（自检自身非恒绿，847 号钩子语义保持）。
- **双引擎真人走查（一次性探针，不入仓库）**：直启双引擎（生产形态参数对齐 node-fallback-smoke）+ 双根布局（Rust 腿 `books/<id>` 原生布局；Node 腿同播种后过**官方 `migrateLegacyWorks`** 注册——697 号契约：2.0 列表面=works 清单，legacy 目录须显式迁移，Node 空列表是契约正确行为非 bug）。走查实证**双引擎行为恒等**：章号索引恒等 `[1,2,3,7]`（超 u32/全角数字文件双端不列章）、同章双正典两腿同选 `0001_开局！.md`（码点序修复活体证明——旧 TS 实现此处会取 emoji 文件）、超界章号双 404。
- **探针教训**：①畸形 `chapters/index.json`（`{}` 无 `chapters` 数组）使 `migrateLegacyWorks` 该项 `invalid`（"chapters.map is not a function"）——逐项隔离、响亮失败，行为合理；②`listBooks` 差异（works 清单 vs books 目录扫描）是 697 号已文档化的刻意设计（fixture 注释 + CLI `work migrate` 官方工具），**非遗漏，勿"修"**。

## 五、教训

- **恒等「当前测试全绿」≠ 恒等全域**：845 号后双端测试互绿，但恒等只在 ASCII/BMP/小章号子域成立；跨引擎一致性要用「找出两引擎语义天然不同的维度」（排序序、数值上界、字符类）主动压测，非靠用例数堆叠。
- **测试断言宽度必须 ≥ 事故形状**：847 的 ≥1 断言覆盖不了单数汇总行分支——每类可区分的输入形状（单/复数、单/多漏洞）应有各自专属夹具，否则该分支的回归对门禁不可见。
- **放行面必须等于定性面**：按模块名放行是「该模块现在这些漏洞可接受」到「该模块未来一切漏洞可接受」的静默扩张；风险白名单一律钉定具体 id，新 id 新定性。

## 六、出口

维持：上游数值分协议扩展 / serde_json >1.0.154 解除 / @streamdown/math katex 0.18 / vitest 4（消除 GHSA-5xrq/82fw/qxg6 三连）/ pi1.0 key / 新真实需求。**用户侧待办：Fork 推送本地累积提交。**
