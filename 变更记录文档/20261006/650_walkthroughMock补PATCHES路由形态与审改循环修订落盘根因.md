# 650 号：walkthrough-mock 补 PATCHES 路由形态——并揪出「审改循环修订产物从未落盘」的 mock 缺分派根因（646 备案清偿）

日期：2026-10-06。类型：chore(scripts 走查基建)。前置：646 号备案「PATCHES 路由未覆盖」。**本号活体定位并修复了一个 646 号起即存在、被复审通过状态掩盖的 mock 仿真缺口**（修订产物从未落盘）。

## 一、PATCHES 路由形态（646 备案主目标）

- 新 env `WALKTHROUGH_MOCK_AUDIT_SCOPE`（默认 structural=646 形态；`local` → 降分 issue 以 `repair_scope:"local"` 注入 → reviser `resolveAutoOutputMode` 判 **patch-only** → system 指令「你必须只输出 PATCHES」）。
- mock 修稿分支按路由指令探测双形态：`includes("只输出 PATCHES")` → `=== PATCHES ===`/`--- PATCH 1 ---`/TARGET_TEXT/REPLACEMENT_TEXT（对齐 parseSpotFixPatches 正则；TARGET 为 WRITER 正文精确句、REPLACEMENT 等义改写，applySpotFixPatches 先精确后空白归一匹配）；其余（rewrite-only/allow-full）→ REVISED_CONTENT 整章形态（646 不变）。
- PATCH 链本地单测自证：parse 1 条/apply true/TARGET 唯一命中/替换句在修订稿。

## 二、活体定位「修订产物从未落盘」（本号最高价值发现）

PATCHES 红绿首跑即撞「循环达标（45→91）但章节落原稿/缺章」——三层排查（runner 临时指纹日志+review-cycle 最小 harness+mock 分派 debug 日志）定位根因链：

1. **review-cycle 与 runner 均正确**（harness：finalContent=PATCH 版 2407/revised=true；runner 指纹：reviewResult.finalContent len=2433 hasRepl=true、≠output.content 走 analyzer 分支）。
2. **真根因=mock 分派表缺「小说连续性分析师」**（buildPersistenceOutput→ChapterAnalyzer，审改循环修订后 finalContent≠初稿时被调）——落入 else 确认卡文本，analyzer 拿到非法载荷后管线**静默悬挂**在「生成最终真相文件」（无校验/同步/索引阶段、无错误输出、write-next 已受理返回 200）——**646 号起任何审改循环修订都无法完成落盘**，章节却因复审 91 分标 ready-for-review（状态与内容错位被掩盖）。
3. 645 时代无此问题=无修稿时 `buildPersistenceOutput` 短路（finalContent===output.content，normalize 对纯正文恒等）不调 analyzer。

**修复**：mock 补「小说连续性分析师」分派（对齐 ChapterAnalyzer 的 === TAG === 输出契约给最小合法形态；content 由 persistenceOutput 强制回写审改后 finalContent 不毁正文）。

## 三、红绿（活体）

- **红**：三遍复现（62,91 / 45,91 ×2）——循环达标后章节缺落/落原稿（2405）、管线静默悬挂于「生成最终真相文件」。
- **绿**（analyzer 分派补齐后）：`修复轮次 1/1（45 分）→ 修复后达到通过线（91 分）` → 全链走完（落盘/校验/同步/更新索引）→ **0002_风起.md 含 PATCH 替换句（旧句移除，2443 字符）=局部修补真实落盘**，区别于 646 整章重写路径（+100 字尾段）。章节 ready-for-review。
- **回归**：默认 structural 形态（未注入/62,91+structural）fixture 全绿零回归（analyzer 仅修稿后被调，无循环路径不触及）。gate:ts 八步全绿。Rust 零触碰（647 刚整体复验）。

## 四、备案

- **analyzer 占位的 truth 保真边界**：UPDATED_* 最小占位使 memory.db promises 投影缺失 → fixture promises 断言红（650g 实录，env 自杀）——AUDIT_SCOPE=local 形态用于管线级验证可用、用于完整 fixture 走查需 analyzer 仿真保真化（UPDATED_HOOKS 从 settler 状态渲染），另号。
- 646 号变更记录的「fixture 修订正文 2487 字」实为 mock 自检字数而非落盘实测——646 的修订同样未落盘，本号溯源时以 0002 正文 grep 为准已纠偏。
- mock-debug 临时日志已移除；runner 临时指纹已还原（git checkout）。

## 五、教训

1. **「状态对」不等于「产物对」**：复审 91 分让章节 ready-for-review，内容却是原稿——验收要下到落盘正文痕迹（grep 替换句），与 644 的「文案对≠形态对」同族。
2. **走查环境静默悬挂最贵的排查成本在「请求到没到」**：mock 分派 debug 日志（一条 stderr）一步定位分派缺失，胜过多轮源码推理——基建脚本的临时调试日志模式（加→实证→移除）应常态化。
3. **core dist vs src 的调试陷阱**：studio server 经 tsx 跑的是 core **dist**——改 core src 加调试日志必须先 rebuild（641 号教训的变体：本次 dbg 首轮无输出即此因）。
4. mock 仿真的覆盖面必须跟着真实链的新增分支走：审改循环引入 analyzer 落盘链时（646 之前即有），mock 分派表没有同步——**分派表与真实 agent 清单的对齐审计应成为 mock 改动的固定步骤**。
