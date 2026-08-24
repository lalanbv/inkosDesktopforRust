# 166 · 终切后补齐：Rust 引擎 updater 通道 + 设置面板后端开关 + 聊天消息级打字机 + 章节读写对照分屏

## 一、背景

承接 164（桌壳默认 Rust 引擎）/165（project 读侧对齐）。本轮按「补齐 Rust 客户端与
桌面客户端所有逻辑和功能 + UI 规划全任务收官」清账三类缺口：

| 缺口 | 出处 |
| --- | --- |
| engine updater 通道仍为 Node bundle 语义（更新没人跑的 Node 包） | 164 §五.1 |
| settings 面板无 backend 开关（只能改 TOML/env） | 164 §五.2 |
| ChatPage 消息级打字机（专注模式仅全局环境层生效） | 162 备案 |
| 章节读写对照分屏（目标布局图「可分屏 ≤2 组」，163 评分 -1 项） | UI 优化方案 §目标布局 |

## 二、实现

### 1. Rust 引擎 updater 通道（src-tauri）

- `BundleFlavor { Node, Rust }`（updater/engine.rs）：
  - 资产名：Node=`engine-{ver}.tar.gz`（既有）/ Rust=`inkos-engine-{ver}-{triple}.tar.gz`
    （package-rust-engine.sh 契约；`rust_triple()` 按 OS/ARCH 编译期映射
    apple-darwin/gnu/msvc 三平台）。
  - 健康预检：`bundle_health(dir, flavor)`——Node=manifest+`dist/index.js`，
    Rust=manifest+`inkos-engine-server`。
  - 解包布局：`flatten_single_top_dir`——Rust tarball 顶层内层目录
    `inkos-engine-{ver}-{triple}/` 展平到替换目录根；平铺布局（Node）原样；
    无 manifest 的垃圾布局不动（健康预检兜底走回滚路径）。
- `EngineChannel.with_flavor()`；默认 Node（既有测试零迁移）。
- main.rs 接线：`UpdaterState` 增 `current_rust_engine_version`（app_data →
  resource 解析序同 rustbin）+ `rust_engine_dir/rust_engine_bak_dir`
  （app_data/engine-rust[.bak]）；`active_flavor()` 按**运行态生效后端**
  （164 补录的 EngineBackendState）分流 check/apply 的版本源与目录组——
  rust（默认）更新 Rust 引擎包，node（回退）维持既有行为。

### 2. settings 面板后端开关（src-tauri/picker/settings.html）

- 引擎区新增「引擎后端」下拉（rust=默认/node=回退），双语 i18n；
  `buildConfigFromForm` 落裸字符串枚举（与 EngineBackend serde 形态一致），
  `populateConfigForm` 旧配置缺键回 rust。update_config 命令链零改动
  （AppConfig 已有字段 + serde 校验）。

### 3. ChatPage 消息级打字机（162 备案闭合）

- 专注模式下点击消息聚焦、其余 `typewriter-dim` 降暗——与 ChapterReader
  段落级同款语义同款实现（store 选择器 + active index 状态），消息包装层
  一处接入，用户/助手消息统一生效。

### 4. 章节读写对照分屏（UI 优化方案目标布局功能落地）

- ChapterReader 分屏组：左=主稿（读/写），右=`SplitChapterPane` 只读对照栏
  （独立拉章，标题行含上一章/下一章/#跳章/关闭；无前章空态提示）。
- 宽度拖拽 260~720 钳制 + localStorage 记忆 + 双击复位 420——复用 P3-2
  `usePanelWidth` 参数化能力（side:"right"），零新拖拽逻辑。
- 工具栏「对照分屏」开关按钮（Columns2 图标，开启高亮）；i18n 双语 8 键。

## 三、验证（全部真跑）

| 面 | 结果 |
| --- | --- |
| src-tauri 全量 cargo test | **574 过 / 0 失败**（updater 新增 7：triple 形状/资产名契约/健康预检分流/展平三态/flavor 构造器） |
| src-tauri clippy `--lib --tests --bins -D warnings` | 零警告 |
| studio tsc --noEmit | 零错误 |
| studio vitest | **720 过 / 0 失败**（81 文件） |
| studio e2e 全量 | **36/36**（新增 reader-split 2 用例：开/关+换章+跳章+错误面、拖宽+重开记忆；focus-mode 等 34 既有零回归） |
| 分屏 e2e 踩坑实录 | ①缺全局桩（project/books/daemon）致 ⌘P 无索引——补 beforeEach 桩对齐 focus-mode 模式；②pane 无 overflow-hidden 时内容 min-width 撑破 style 宽（布局宽≠state 宽）——加 `overflow-hidden` 严格贴合；③分隔条在长稿深处 y=812 超出 720 视口、鼠标事件不命中——`scrollIntoViewIfNeeded` 后拖拽生效（SidePanel 既有用例无此问题因其贴视口顶部） |

## 四、完成度对照（目标三要件）

1. **UI 规划全任务**：150 号四期 25 工作包 163 号收官（十维 4.2≥4.0）；本轮补齐
   布局图功能项「分屏 ≤2 组」与 162 备案「消息级打字机」。规划文档内再无
   未实现功能项（axe 集成为「边际按需」裁量、工作区预设从未排期——均非遗留任务）。
2. **Rust 客户端（engine-rs）**：115 号 v3 终版两轮穷尽复核确认功能/协议/配置/
   提示词/中止/通知/检测/磁盘/SSE 九面对齐；仅余 pi-ai 模型卡元数据与
   authoring-store 边角两项「无行为面/无端点消费面」备案（非缺口）。
   165 号补齐读侧最后一处契约差。
3. **Rust 桌面客户端（src-tauri）**：164 终切 + 诊断回显 + 本轮 updater 通道/
   设置开关——功能面齐。遗留均为发布决策类：v0.3.0 双引擎资源瘦身（回退期
   建议双留）、tag/Release 上传等外向动作（145 号先例留用户）。

## 五、关联提交

- feat(desktop): Rust 引擎 updater 通道（BundleFlavor）+ settings 面板后端开关（166 号）
- feat(studio): 聊天消息级打字机 + 章节读写对照分屏（166 号）
