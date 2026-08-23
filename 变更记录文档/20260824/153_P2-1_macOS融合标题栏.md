# 153 号变更:P2-1 macOS 融合标题栏(Overlay + 顶栏拖拽区 + 交通灯留白)

- 日期:2026-08-24
- 模块:src-tauri(桌面壳)/ packages/studio(顶栏)
- 类型:桌面融合层功能(P2 首个工作包,151 号分解)
- 关联:150 号(总纲)、152 号(P1,顶栏三段化与本变更的前置)

## 一、变更内容

### 1. Tauri 壳侧

- **N `src-tauri/tauri.macos.conf.json`**(平台覆写文件,仅 macOS 构建合并):
  - main 窗口 `titleBarStyle: "Overlay"` + `hiddenTitle: true`——系统标题栏条消失,红黄绿交通灯悬浮于内容之上,窗口标题文字隐藏
  - **平台覆写文件的 windows 数组是整体替换而非按 label 合并**,故条目内 width/height/title 全量重申;后续若改基础 tauri.conf.json 的 main 窗口字段,须同步此文件(漂移风险已在此备案)
  - Windows/Linux 不合并此文件,保持系统标题栏(151 号既定分支策略)
- **M `src-tauri/capabilities/main.json`**:权限增加 `core:window:allow-start-dragging`——Tauri v2 内建拖拽脚本对 `data-tauri-drag-region` 的 mousedown 会 invoke `plugin:window|start_dragging`,无此 ACL 授权时**静默失效**(146 号同类教训:内建注入脚本的调用同样过 ACL)
- **M `src-tauri/picker/index.html`**:body 顶部新增 38px 全宽不可见拖拽条(`data-tauri-drag-region` + `position:fixed`)——Overlay 后 picker 阶段(项目选择/启动中)否则没有任何移窗区域;双击最大化由 Tauri 拖拽脚本内建处理

### 2. Studio 前端侧

- **N `packages/studio/src/lib/titlebar.ts`(纯函数,全量单测)**:
  - `isMacPlatformAgent(platform)` / `isTauriRuntime(globalScope)`(判 `__TAURI_INTERNALS__`,浏览器开发模式为 false)
  - `deriveHeaderInsetClass(mac, tauri)`:仅 macOS+Tauri 壳内返回 `pl-[78px]`(交通灯横向占位,151 号规格值),其余(含浏览器与 Win/Linux)`pl-8`
- **M `packages/studio/src/App.tsx`**:顶栏启用 P1 预留锚点——header 根 + 面包屑 nav + 中段搜索容器 + 右段控件容器四个节点标注 `data-tauri-drag-region`;`px-8` 改 `headerInsetClass + pr-8`
  - 拖拽语义依据:Tauri 拖拽脚本只认 **mousedown 目标元素自身**的属性,按钮/输入框/kbd 徽标等子元素不带属性即天然可点,无需额外 no-drag CSS(151 号"可选"项判定不需要)
- **M `packages/studio/src/components/AppShellSkeleton.tsx`**:启动壳 header 同样处理——启动骨架与就绪布局左缘一致,切换无跳变
- **测试**:`lib/titlebar.test.ts`(4 用例:平台矩阵/运行时判定/留白四组合/类名与常量同步)+ `AppShellSkeleton.test.tsx` 增拖拽区与默认留白断言

## 二、验证链

| 门禁 | 结果 |
| --- | --- |
| studio vitest | 70 文件 / 646 测试全绿(新增 6) |
| typecheck(client + server) | 0 错误 |
| `cargo check`(src-tauri) | 通过——tauri-build 校验并合并 tauri.macos.conf.json 与 capability |
| 构建产物核查 | `target/.../out/capabilities.json` 含 start-dragging;studio dist 资产含 `data-tauri-drag-region` 与 `pl-[78px]` 分支 |
| 桌面壳实启 | `cargo run` 起动正常,日志干净到达 picker 阶段(窗口以 Overlay 配置创建成功,配置非法会在构建期失败) |
| `cargo test`(src-tauri) | 通过(exit 0:单测+集成+doctest 全绿;本变更未动 Rust 源码,系 P2 期首跑基线) |

## 三、真机验证缺口(环境受限,待人工)

本会话运行环境未获 macOS「辅助功能/屏幕录制/自动化」授权,以下项**无法机器验证**,须真机人工走查(全部在 dev 构建上):

1. 顶栏(面包屑区/中段空隙/右段间隙)按下拖动 → 窗口移动;
2. 双击顶栏 → 最大化/还原;
3. 交通灯(左上三键)不遮挡「首页」面包屑首段(78px 留白);
4. 顶栏全部控件可点:面包屑回跳、⌘K 搜索框、中/EN、主题按钮;picker 阶段:顶条拖动移窗、选择目录按钮可点;
5. Windows/Linux 冒烟:系统标题栏原样(不合并 macos 覆写)。

回滚:删除 `tauri.macos.conf.json` + capability 一行 + 还原 App/AppShellSkeleton 的属性与留白(151 号既定)。

## 四、连带发现

- src-tauri/README 声明**本仓必须 pnpm 10**(pnpm 11 不读 package.json 的 pnpm.overrides)。核查结论:152 号期间 pnpm 11 重锁的 pnpm-lock.yaml **安全**——packages/core 对 @mariozechner/pi-agent-core/pi-ai 本就是精确版本 specifier(0.67.1),overrides 属冗余保险,锁定结果两版一致,且 `pnpm install --frozen-lockfile`(pnpm 11)已在本次构建脚本中通过。pnpm-workspace.yaml 的 allowBuilds 迁移(152 号)已消除 pnpm 11 的交互清装行为。
