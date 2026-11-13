# 1010 号：托盘菜单面双语清偿轮——locale 上报联动 refresh + BookDetail key 防御行

日期：2026-11-14
范围：`src-tauri/src/lifecycle.rs`（build_menu/tooltip 双语化 + refresh_locale）+ `src-tauri/src/main.rs`（cmd_set_locale 联动）+ `packages/studio/src/App.tsx`（BookDetail key 防御行）
前置：1009 备案②（托盘菜单面未普查）+ 1008 备案（BookDetail 无 key）
门禁：cargo test -p inkos-desktop 25 目标 0 FAILED（lib 474 含 +1）+ studio 893/893 + clippy:gate 双 crate 零告警 + U+FFFD 净

---

## 一、托盘面普查与清偿（1009 备案②转正）

- **直显面清单**：托盘菜单 4 项（badge 灰显项「未读: N/未读: 0」、show「显示窗口」、plugins「插件管理…」、quit「退出」）+ Linux set_menu 失败时 tooltip 兜底串「inkosDesktop · 未读 {n}」。
- **双语化**：build_menu 四项 → `pick`；tooltip → `fallback_tooltip()`（Locale 匹配双语）；badge 串抽纯函数 `tray_badge_label`/`pick_owned_badge`（供单测双形态锚定）。
- **时序约束与平台边界**：菜单在 setup 构建一次（早于前端 locale 上报），但 macOS/Win 每次角标变化都会 set_menu 重建——**locale 上报后触发一次 refresh 即生效**；Linux GTK once-set 限制下菜单/tooltip 保持初始形态（备案，与 M2 fix 的既有平台边界一致）。
- **联动**：cmd_set_locale 加 `State<TrayController>` 参数，locale 真变化时调 `refresh_locale()`（清 500ms 去抖窗口 + refresh best-effort log）。

## 二、BookDetail key 防御行（落 1008 备案）

App.tsx BookDetail 挂载点加 `key={view.bookId}`：跨书 in-place（book-settings(A)→(B) 不卸载）当前 UI 不可达，但 BookDetail 的惰性 useState 初始化器（草稿覆盖层恢复 `loadBookSettingsDraft(bookId)`）只跑一次——in-place 换 bookId 会**跨书串扰覆盖层**。id 键页按 id 重挂载是防御基线，本号落行封口潜在类。

## 三、测试与教训

- `lifecycle.rs`：tray_badge_label_follows_locale（zh/en 双形态锚定，持 locale 锁——1009 号 test_locale_lock 惯例直接复用）。
- **教训：lib crate 内不能 `inkos_desktop::` 自引用**（bin 专用路径）——lifecycle.rs 属 lib 树，一律 `crate::locale::`；E0433 编译期即拦。
- Mimosa PreToolUse 双拦（perl -pi 写源码）按例走 Edit replace_all（847 号门禁形态：Bash 写源码被拒走 Write/Edit）。

## 四、下一号自 1011 起

出口常备：回归审查轮（996-1010 新增面全量走查候选）/ BookTimeline 等同族 key 备案（其它 id 键页挂载点普查）/ 用户侧待办=Fork 推送（含 955-1010）。
