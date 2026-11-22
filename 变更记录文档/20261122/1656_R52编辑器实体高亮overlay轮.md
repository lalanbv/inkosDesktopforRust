# 1656 号：R52 编辑器实体高亮 overlay——命中集纯函数双端 golden + 镜像层组件 + IAB 四轴走查

> 日期：2026-11-22 ｜ 类型：功能轮（v8 深化三 R52 全量落地，1655 施工输入评估既定三步顺序执行；引擎零改动——命中集走既有 Codex 名册端点）
> 定位：编辑器 overlay 三件套=①命中集扫描纯函数（Codex tracking 语义）②textarea 镜像层+悬停实体卡 ③IAB 像素对齐活体走查。

## ① 命中集扫描纯函数（golden `entity-highlight-vectors.json`，8 向量，TS 生产派生）

- `scanEntityHighlights` ↔ `scan_entity_hits`（Rust `utils/entity_highlight.rs` **纯 golden 纪律镜像，零引擎接线**——R52 是纯前端功能 S 为主）。
- 语义（v8 R52 定义）：**大小写敏感**（indexOf 无 flag，中文恒命中）+ **排除短语条目级**（exclude 名单精确匹配术语）+ **重叠最长优先**（别名「林动大哥」压过名「林动」，同长同位先注册卡先得）+ minLength 缺省 2（单字噪声过滤）+ **码点下标口径**（golden_search_highlight 家族同款，astral 向量锁定）。
- 向量组：zh-multi-hit/en-case-sensitive（小写不亮）/longest-alias-wins/overlap-greedy-cross-card/exclude-phrases/min-length/astral-codepoint-offsets/empty。
- 测试：TS 锚面 9 测 + Rust 差分 1（golden_entity_highlight_diff.rs）+ Rust 模块内 2。

## ② 镜像层组件 EntityHighlightOverlay（ChapterReader 编辑态挂载）

- **流式镜像方案**：textarea（bg-transparent，z-10）背后 absolute inset-0 镜像 div（同文本质感渲染，普通段 `text-transparent`+命中段 `bg-primary/25 rounded`，pointer-events-none 除命中段）——正文由 textarea 渲染=零双影；soft wrap 由浏览器排版（零手动行高度量）。
- **同源样式单源化**：`EDITOR_TYPOGRAPHY_CLASS`（font-serif text-lg leading-[1.8] p-6）单源导出，ChapterReader textarea 同引——字体/行高漂移错位风险收口。
- **滚动同步**：textarea scroll 事件 → mirror scrollTop/scrollLeft 直写。
- **大章双闸**：`ENTITY_OVERLAY_MAX_CHARS=20000` 码元禁用（mirror DOM+扫描全停）+ `useDeferredValue` 编辑防抖（快速键入命中集滞后一拍）。
- **悬停实体卡**：命中段 mouseenter → tooltip 浮层（name/kind/summary/facts≤3/别名，只读）。
- 集成：ChapterReader 编辑态 wrapper（data-editor-overlay-host）+ Codex 卡拉取（编辑态一次，GET /books/:id/codex 既有端点零改动）+ 全局开关 localStorage `inkos.entityHighlight`（默认开）。
- 组件测 4：命中渲染（最长别名+大小写敏感+镜像全文拼接=原文+text-transparent 防双影）/悬停卡出入/大章闸（>2 万 off+零命中 DOM）/空输入。

## ③ IAB 走查（Rust 引擎 4569 + vite dev 4567，e2e-a11y-book 加 entity_codex.json 三卡 fixture）

- **像素对齐 ✓**：mirror 与 textarea 同起点同宽（397,355,740；高差 8px=scrollbar 保留区，不影响文本）；命中段几何自洽（padding 24+行高 32.4 级数）。
- **滚动同步 ✓**：scrollTop 300→300 精确；183 命中段长文在线。wheel+手动 scrollTop 双通道叠加时 9px 伪影（真实用户单通道滚动不触发）备案非缺陷。
- **悬停卡 ✓**：mouseover 合成事件触发 tooltip（role=tooltip，summary/facts/别名全渲染）——React onMouseEnter 需冒泡 mouseover 形态（mouseenter 直派不触发，走查实证）。
- **视觉 ✓**：截图分析确认高亮渲染、基本对齐、布局无破。**备案**：命中段 span 跨 soft-wrap 行时圆角切割不优雅（inline 元素跨行形态）——`box-decoration-break: clone` 优化挂观察。
- **帧率锚点：环境限制备案**——IAB evaluate 为隔离世界（window 状态不跨调用保持，rAF 采样不可达）+ CUA 滚动手势本环境超时；机制性保障=useDeferredValue 防抖+大章双闸+纯函数 O(术语×文本) 有界。真机帧率锚点挂后续轮 bench 前端专项。

## 轮内勘误

- 命中段切片首版用 `deferredText.slice`（UTF-16 下标）与码点口径 hit 错位（astral 文本）→ 改 `Array.from` 码点数组取段（口径一致性=golden 同源纪律在组件层的延伸）。
- sort comparator 笔误（长度差恒 0）——`(b.end-b.start)-(a.end-a.start)` 修正。
- 组件测又踩 fake timers+waitFor/useDeferredValue 调度死锁（1653 教训二犯）→ 真实时钟。
- 走查中断恢复：会话 goal 状态恢复后 IAB 标签页失联（controlled+user 双列表空）→ 重开页面续走查（服务存活先验证）。

## 门禁记录

- cargo:testgate 全绿：audit **双 0**（轮内今日新披露 **RUSTSEC-2026-0592**（url `Url::origin()` 嵌套 blob: URL 栈溢出）即时清偿——engine-rs/src-tauri 双 crate `cargo update -p url --precise 2.5.9`，advisory-db 0 天前=当天披露当天修）+ 必红自检活体（9+1）+ engine-rs **68 目标 2190 passed**（+1 目标=golden_entity_highlight_diff，+3 测）+ src-tauri 600；INKOS_DUEL=1 真跑。
- gate:ts 十腿全绿（终跑；中程 test 腿一红=ChapterReader.interaction 既有 fetchJson mock 未覆盖 /codex 路径返回 undefined → mock 补 `{cards:[]}` 缺省（测试环境不全补全，非迁就实现））。
- fmt（--check 幂等）+ clippy --all-targets 双 0；U+FFFD 净。

## 出口

- R52 收口（三步全落+IAB 四轴活体）；优化观察项：box-decoration-break 跨行切割/书级开关 UI/悬停卡跳转 Codex 编辑。
- 下号自 1657 起；出口=R53 章历史浏览面（versions 三端点双端已在位+LCS 复用，S 为主降级面）。
