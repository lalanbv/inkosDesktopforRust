# 156 号变更:P2-7 macOS 菜单栏对齐(菜单事件汇入命令注册表)

- 日期:2026-08-24
- 模块:src-tauri / packages/studio
- 类型:桌面融合层功能(151 号 P2 末个工作包;P2 七个工作包至此全部完成)
- 关联:153/154/155 号(P2-1~6)

## 一、变更内容

### Rust 侧(main.rs)

- N `build_main_menu()`:macOS 主菜单五段——
  - **应用**(系统预置:关于/服务/隐藏/隐藏其它/全部显示/退出;缺失会丢标准行为)
  - **文件**:新建小说(`CmdOrCtrl+N`,唯一带 accelerator 的命令项)
  - **编辑**:系统预置 undo/redo/cut/copy/paste/select_all(webview 内由 macOS 补齐)
  - **视图**:命令面板 / 浅色·深色·跟随系统主题 / 中文·English(均不带 accelerator)
  - **窗口**:最小化/最大化/关闭(预置)
- setup 末尾 `app.set_menu(menu)`;`.on_menu_event` 把菜单 id 以 **`menu://command` 事件** emit 给 main 窗口——菜单项不携带命令语义,前端映射执行(单一事实源原则)
- **accelerator 策略**:除 ⌘N 外不注册 OS 级菜单快捷键,避免与 Web 内 HOTKEY_DEFS 双注册互相拦截(151 号"菜单快捷键与 Web 内一致"由同源注册表保证)

### Studio 侧(App.tsx)

- 快捷键 runner 重构为 `dispatchCommand`:处理 `menu:` 前缀 → `MENU_COMMAND_MAP` 映射(7 项:new-book/palette/theme×3/lang×2)→ 复用命令注册表/应用动作分发——菜单、快捷键、命令面板三入口同源
- Tauri 菜单事件监听:仅 Tauri 壳内(`__TAURI__.event.listen`),`menu://command` → dispatchRef(每渲染刷新,无过期闭包);事件通道不可用时静默降级

## 二、验证链

| 门禁 | 结果 |
| --- | --- |
| `cargo check` + `cargo test`(src-tauri) | 通过(exit 0) |
| 桌面壳实启 | `cargo run` 带菜单启动正常,到达 picker 无 panic(set_menu 失败会中断 setup) |
| studio vitest | 70 文件 / 665 测试全绿 |
| typecheck(client + server) | 0 错误 |
| e2e 全量 | **23/23 全绿**(command-palette 2 + loading-skeletons 4 + quick-open 2 + 存量 15) |

## 三、真机验证缺口(与 153 号同因,待人工)

菜单点击 → 命令执行、⌘N 新建小说、菜单与 Web 行为一致性,需 macOS 真机走查(会话环境无辅助功能/自动化授权)。清单并入 153 号第三节。

## 四、P2 批次总结(08-24,提前于 08-31 排期)

| 工作包 | 状态 | 记录 |
| --- | --- | --- |
| P2-1 macOS 融合标题栏 | ✅(OS 级验证待人工) | 153 |
| P2-2 窗口状态记忆 | ✅(同上) | 154 |
| P2-3 主题 auto 三态 | ✅ | 154 |
| P2-4 快捷键分发器 | ✅ | 155 |
| P2-5 ⌘P 快速打开 | ✅ | 155 |
| P2-6 ⌘/ 速查弹层 | ✅ | 155 |
| P2-7 macOS 菜单栏 | ✅(OS 级验证待人工) | 156(本文) |

P2 收尾既定节点(08-28):全平台门禁复跑、三平台冒烟、打包 v0.2.0-alpha.2、启动掐表基线(与 v0.2.0-alpha.1 同节点)。
