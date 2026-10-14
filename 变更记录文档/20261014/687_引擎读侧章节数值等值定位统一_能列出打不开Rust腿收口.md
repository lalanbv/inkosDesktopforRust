# 687 号：引擎读侧章节数值等值定位统一（「能列出打不开」Rust 腿收口）+ 全量矩阵复验 + 模拟真人 GUI 走查

日期：2026-10-14。类型：fix(engine-rs)+fix(scripts)。前置：686 号（HEAD 35785c8e）。

## 一、缺口发现（全量测试+代码审查+GUI 走查轮）

本号目标=683 号形态的全量复验轮：全量测试矩阵 + 代码审查 + 模拟真人浏览器黑盒走查。走查活体抽查（node fetch 探针 + 浏览器 UI 双路径）在 walkthrough 夹具（683 号故意播种的 `0001-镜中醒来.md` 连字符种子 + `7_手记.md` 单数字考题）上抓获 **Rust 引擎读侧 8 处消费点未镜像 682 号 TS 修复**：

- `GET /api/v1/books/:id/chapters/7`（文件 `7_手记.md`）→ **404**，而 book_detail 章节列表照常列出第 7 章——「能列出打不开」在 Rust 腿完整存活；
- 前缀碰撞（682 号根除的形态）在 Rust 反向更危险：仅 `00071_x.md` 存在时 `chapters/7` 旧码 `starts_with("0007")` → **200 命中错误章节内容**（TS 侧 682 后为数值等值 404）；
- 同号双形态（`0001-` + `0001_` 并存）旧码取 read_dir 首个命中——目录序非确定，跨重启/跨平台内容可能翻转。

根因=682/683 号把 TS 读侧 7 处消费点统一到 `findChapterFileByNumber`（core/state/chapter-filename.ts 单一事实源）时 **Rust 引擎侧读面未随动**（683 号收口了 writer 删侧镜像，读侧 8 处 `format!("{:04}")` + `starts_with` 首匹配残留）。

## 二、修复（TS `chapter-filename.ts` 逐字镜像）

新增 `engine-rs/src/state/chapter_filename.rs`：

- `parse_chapter_file_number`：`^(\d+)[_-]?(.*)\.md$` 前导号解析，数值须 >0（`0000_`/非章节 md 排除）——对齐 `parseChapterFileName`；
- `find_chapter_file_by_number`：数值等值过滤 → 字典序排序 → `^0{3}\d`（padded-4 引擎主形态）类优先、类内字典序兜底——对齐 `findChapterFileByNumber`（**语义裁决细节：同号双形态同属 padded 类时类内字典序取 `-` < `_`，即 `0001-` 种子胜出——「确定性」是目的而非「下划线规范名」，与 TS 实现及 core 单测 L41/42 逐字核对**）；
- `resolve_chapter_file(chapters_dir, n)`：read_dir + 定位便捷形态，目录缺失/IO 错误归 None 由调用方落各自 404/软失败语义。

**换装 8 处读面**（逐一核对 TS 对应端点均为 682 后 find 形态）：

1. `books_state_routes::read_chapter`（GET chapters/:num，server.ts L4005）
2. `books_state_routes::post_workspace_inspiration`（L4067）
3. `books_state_routes::detect_chapter`（L7293；旧 `"00-1"/"NaN"` padded 模拟随 startsWith 一并退役——负数/NaN 无前导号等值候选自然 404）
4. `audit_route` 审计读章（683 号 unified 消费点）
5. `books_routes` 修稿链读章（reviseDraft）
6. `books_routes` 状态降级修复读章（scheduler 副本）
7. `books_routes` 持久化章 sync 读章
8. `ops_routes::run_detection`（readChapterContent）

**写侧不动**：`write_next::find_persisted_chapter_file` 的 `starts_with("{padded}_")` 是 683 号裁决过的引擎自产工件定位（writer.saveChapter superseded 清理逐字镜像），维持。

## 三、红绿闭环与回归锁

- **单测**：`chapter_filename` 2 测（解析语义/类优先+字典序+碰撞根除）；
- **e2e**：`chapter_read_numeric_equivalence`（books48 套件）：单数字 `7_手记` 200 + `00071` 归 71 + 双形态确定性取 `0001-`——红绿=旧码 `chapters/7` 404 → 新码 200；
- **活体**：walkthrough 环真实引擎，修复前 `chapters/7` 404 → 修复后 `200 {"filename":"7_手记.md"}`；GUI 走查「07 手记」点击 → 阅读器完整打开（修复前 404 空态），截图留档 `gui-test-screenshots/t8_verify_*.png`；
- **契约差分入列锁回归**：`/books/${BOOK}/chapters/1`（683 号故意播种的 `0001-` 种子 + resync 产 `0001_` 并存面）——filename+content 双腿字节级恒等，**101 端点 0 分歧**。

## 四、本号全量矩阵（修复后终态）

- cargo:testgate：engine-rs 45 目标 **1933 passed**（基线 1930 + 单测 2 + e2e 1，零漂移）+ src-tauri 25 目标 **584 passed**（INKOS_DUEL=1 真跑；一次复跑 `strangler_cross_write_read_duel` 60s 未就绪红=走查环境+浏览器未清的负载竞态，隔离复跑 10/10 41s 绿 + 全量复跑绿=既有基建竞态非回归，685 号同型三步法收口）
- clippy 双 0 / gate:ts 八步全绿（contract-diff 步=101 端点）/ 矩阵双腿 7/7×2 / studio e2e 36/36 / bindings 186 / manifests 3 包 / semantic 0
- bench：**静默窗重试环捕获窗口后门禁通过**（loadavg 340‰<350‰ 阈，全基准带内 title_dedup +1.7%/tool_registry ±5%）——本号改动全为 axum 路由读面，未触碰任何 criterion 基准组函数（hot_paths 七组全为纯库函数）；当日午后两次门禁红（dispatch/1 +72%/+207%，std 51-100）经隔离复跑（121.3ns std 1.4）+ 全序直跑（124.3ns）+ 静默窗全门禁绿三重证据定性为负载形态（脚本头注自证 load≈0.23/核即出 +190%~520% 假回退），负载闸拦截=设计行为，重试环保跑而非 `--ignore-load` 洗门禁。

## 五、走查备案（模拟真人黑盒，P0-P2 全过）

首页（遥测累计 10 与 fixture 吻合/SSE 实时徽章）→ 书工作台三面板 → 双章阅读器（首字下沉排版/状态栏字数）→ ⌘K 命令面板（过滤命中）→ ⌘P 快速打开（直达第 2 章）→ 承诺账本（4 承诺 kinds 与 fixture 吻合+分类筛选收窄）→ 影游工坊五步向导+结构图渲染+**试玩全流程**（选择/变量 trust 0→1/mirrorKey 翻转/结局解锁 1/2）→ 聊天回环（mock LLM 应答渲染）。环境备案：WindowServer 高载期合成键盘输入偶发丢字（「章节/风起」成功、后续输入未落）——坐标点击与 fill 路径稳定，属环境负载形态非应用缺陷；本地 curl 对多字节百分号编码路径有解码怪癖（node fetch/undici 同形态全绿=契约客户端无恙）。

## 六、教训

1. **跨端统一修复必须双腿盘点**：682 号修 TS 时备案「Rust 零触碰」，但「读侧统一」的对称义务（引擎读面镜像同一解析）漏了——683 号收口了删侧镜像，读侧 8 处成为跨号漏网；修复跨端契约时应在当号 grep 对端同型消费点并出清，而非留给下一个盲区审计。
2. 「能列出打不开」的 Rust 形态比 TS 更危险：`starts_with("0007")` 前缀碰撞是 **200 错误内容**（数据完整性）而非 404（可用性）——严格化不总是收紧方向，数值等值才是根除。
3. 同号双形态的择优语义要读实现而非顾名思义：TS「4 位补零优先」是**类优先+类内字典序**（`0001-` 胜 `0001_`），按「规范名优先」望文生义会写出看似合理实则分叉的镜像。
4. 负载窗三件套再次生效：负载闸拦截=设计行为；隔离复跑+全量复跑+代码因果三证据定性，禁止 `--ignore-load` 洗门禁。
