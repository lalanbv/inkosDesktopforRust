# 1009 号：桌面壳直显面双语机制收口轮——Rust 侧 locale 奠基 + picker 字典外残留清偿

日期：2026-11-14
范围：`src-tauri`（locale.rs 新建 + error.rs/main.rs/notifier.rs/plugin/commands.rs 双语化 + cmd_set_locale + 残留卫兵）+ `src-tauri/picker`（index.html/settings.html 字典外残留清偿 + locale 上报）
前置：1006 号备案「src-tauri 桌面壳 CJK 无语言机制=专项候选（INKOS_LOCALE 先例可复用）」转正
门禁：cargo test -p inkos-desktop 全目标全绿（lib 473/473，+6）+ clippy:gate 双 crate 零告警 + U+FFFD 净 + engine-rs 零改动

---

## 一、普查与定性（1006 备案转正）

- **INKOS_LOCALE 先例定位**：`packages/cli/src/localization.ts` + `tui/i18n.ts`（env INKOS_TUI_LOCALE→INKOS_LOCALE→LC_ALL 链 + normalizeLocale）——但桌面壳 locale 的**正确信号源是前端 `navigator.language`**（与 picker 前端字典同一判据），env 链不适用（打包 app 的 webview locale 不随 env）。
- **picker 前端已有 i18n 机制**（「i18n 渐进实施」注释）：中英字典 + LANG 检测 + data-i18n 自动填充——缺口=①渐进实施欠账（字典外残留 index ~13 处 + settings ~35 处，JS 动态字符串为主）②**Rust 侧直显面无语言机制**（dialog 标题/系统通知/AppError message/suggestion）。
- **政策边界定性（核心判断）**：AppError 的 message/suggestion/通知标题/dialog 标题是**纯展示面**——前端按 `kind` 分类渲染，从不解析 message 内容判定 → 双语直出，不迂回「EN 基准+前端映射」（1004/1005/1006 政策针对的是**协议判定面**，见 1005 号「判定面 vs 展示面分离」）。对齐 cli TuiCopy 双语表形态。

## 二、修复面

### A. Rust 侧（src-tauri）

1. **新建 `src/locale.rs`**：`Locale` enum（Zh/En）+ `AtomicU8` 全局态 + `set_locale`/`current_locale`/`pick(zh,en)`（&'static 形态）/`pick_owned(zh,en)`（动态 format! 串形态）+ `Locale::parse`（en 前缀判据，非 en 均 zh——桌面壳仅双语）。默认 Zh=原行为零漂移。
2. **error.rs 双语化**：4 个 helper suggestion（network/config/keychain/internal）+ From 系列 ~12 直显字面（io 3+1suggestion、serde_json、tauri、updater 1+1s、keyring 4+1s+1details）。details 中固定中文前缀同步双语化，动态片段（io 错误原文）保持。
3. **main.rs**：cmd_pick_project_dialog dialog 标题、cmd_choose_project 错误 message/suggestion/details 前缀、cmd_apply_engine_update/cmd_apply_shell_update 各 4-5 处 + 2 处 Ok(format!) 直显返回值（settings 页 toast）→ pick/pick_owned。
4. **notifier.rs**：3 处系统通知标题（write:complete/book:created/agent:complete）→ pick（body 透传引擎事件数据，不在本号域）。
5. **plugin/commands.rs**：插件管理窗口标题 → pick。
6. **cmd_set_locale** Tauri 命令 + invoke_handler 注册（前端启动上报）。

### B. picker 前端

7. **index.html**：残留 13 处清偿进字典（showStructuredError 默认参数/操作失败回退/技术细节前缀/全局错误双结构/收藏区三处/插件入口/初始化错误结构/TYPE 表 generic）——TYPE 表改 t() 取值函数化；启动 `invoke("cmd_set_locale")` 上报。
8. **settings.html**：残留 ~35 处清偿进字典（CAP 表→capLabel() 函数化、SENSITIVE_CAPS→sensitiveCaps() 函数化、徽章/切换/卸载/安装/加载失败/遥测指标四串模板化 replace、诊断占位与失败串、剪贴板、全局兜底）+ `<title>` JS 化 + lang 标注同步 + metrics/diag 占位 data-i18n 化 + locale 上报。

### C. 卫兵

9. **picker 残留扫描测试**（locale.rs tests）：零预算判据=含 CJK 且非字典行/非注释/非 console，扫描域=`<script>` 块内。台账判据盲区备案：JS 对象字面量直显（`key: "中文"`）与字典行同形被豁免——picker 域已函数化清零，新增时禁用该形态。

## 三、红绿与教训

1. **NEUTER1009**：pick 强制恒 zh → 恰红 3/3（error 双语/locale pick/notifier 双语——三测试锚定修复本体）→ 互逆恢复。
2. **并行测试全局态竞态（实测坑）**：LOCALE 是进程级单例，cargo test 并行线程下「设 locale→断言」窗口被交错——test_error_serialization 单跑过/全跑挂。修=locale.rs 暴露 `test_locale_lock()`（cfg(test) pub(crate) Mutex），所有设 locale 或断言 locale 敏感文案的用例持锁到用例结束（error.rs 3 + notifier.rs 3 + locale.rs 2）。教训：**全局单例 + 并行测试 = 持锁是惯例不是选项**。
3. **pick 类型签名教训**：`pick(&'static str, &'static str)` 不吃 format! 串——动态拼接走 `pick_owned`（两支都求值，调用点稀少可接受）。
4. **卫兵判据两轮校准**：①首版 filter 谓词逻辑写反（豁免链当保留链——恰红在「合规行全被报」形态上，符合预期红才见怪）；②HTML/CSS 多行注释中间行行级不可识别 → 裁扫描域到 `<script>` 块内（残留风险面=JS 动态串，HTML 静态面由 data-i18n 机制管）。
5. **guard panic 污染全局 panic hook**：crash.rs 的 panic hook 测试在 guard 测试 panic 时连带红（dump.payload 被污染）——guard 修绿即消；形态教训：任何测试 panic 都会进 crash hook 管道，门禁红时看连带面先归因。

## 四、测试（+6）

- `locale.rs`：默认 Zh/pick 双语切换/parse 判据/残留卫兵（4）
- `error.rs`：test_direct_display_text_follows_locale（zh→en 往返 + io 转换双语）
- `notifier.rs`：title_mapping_follows_locale（en 形态锚例；zh 形态由既有用例锚定）

## 五、台账与备案

- **豁免面**：eprintln!/anyhow bail/tracing 日志面（终端与日志文件，非 UI 直显，1004 判据豁免）；with_details 动态片段（io 错误原文）；console.* 前端日志；注释。
- **备案**：①notify body 透传引擎事件数据（ev.data）——数据面非文案面，不双语；②tray 菜单文案（lifecycle.rs build_menu）——本号未普查托盘面（macOS 菜单栏直显），下一专项候选；③JS 对象字面量直显形态=卫兵判据盲区（台账见上）；④Rust 侧 locale 仅 Zh/En 双语，ja 等回落 zh（与前端字典范围一致；桌面壳扩展语种时两侧同步）。

## 六、下一号自 1010 起

出口常备：回归审查轮 / src-tauri 托盘菜单面双语普查（备案②）/ BookDetail key 防御行（1008 备案）/ 用户侧待办=Fork 推送（含 955-1009）。
