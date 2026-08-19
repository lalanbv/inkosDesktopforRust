# 146 · Phase 3 · 客户端 ACL 修复：notification 插件注入脚本误报「操作失败」

> 承接 145 号（0.1.0 发布打包）。用户报告：客户端弹出「操作失败 / 💡 请重试此操作 / 技术细节: Command plugin:notification|is_permission_granted not allowed by ACL / 重试」。

## 一、根因（逐层定位）

1. 错误横幅来自 picker 的**全局 unhandledrejection 捕获**（`window.addEventListener("unhandledrejection")`）——不是任何用户操作链路。
2. 但 picker / settings.html / studio 前端**均未调用** notification API（全库 grep 排除）。
3. 真凶在插件源码：`tauri-plugin-notification 2.3.3` 的 **`init-iife.js`**——插件向每个窗口注入的初始化脚本，它：
   - 用插件命令 polyfill `window.Notification`（Web Notification API）；
   - 尾部 IIFE 在非 Windows 平台**无条件调用** `plugin:notification|is_permission_granted` 以同步 `window.Notification.permission`（悬浮 Promise，无 catch）。
4. 本应用 capabilities 为空（自研命令默认放行，故功能一直正常）→ 该插件命令被 ACL 拒绝 → 悬空 Promise 拒绝泄漏为全局 unhandledrejection → 被渲染成吓人的「操作失败」横幅。**每个窗口每次加载必现。**

## 二、修复

### 1. 新增 `src-tauri/capabilities/main.json`（根治）

```json
{
  "identifier": "main-windows",
  "windows": ["main", "plugin-manager"],
  "permissions": ["notification:default", "dialog:default"]
}
```

- `notification:default`：插件官方默认权限集（16 条命令，含 init 脚本所需 `allow-is-permission-granted` 与 polyfill 后续会用到的 `allow-notify`/`allow-request-permission`）。
- `dialog:default`：**同类隐患预防**——审查发现 `tauri-plugin-dialog` 的 init-iife.js 覆写 `window.alert`/`window.confirm` 为 `plugin:dialog|message|confirm` 调用；页面一旦使用 alert/confirm 即触发同款 ACL 拒绝。一并授权（updater 插件无注入脚本，无需处理）。
- 窗口 label 精确匹配：`main`（tauri.conf.json 主窗）+ `plugin-manager`（运行时 `open_manager_window` 创建，capability 按 label 匹配与创建时机无关）。仅 `local: true` 本地页面——导航后的 sidecar 远程页（127.0.0.1）不授予。

### 2. 横幅文案优化（picker/index.html）

`showStructuredError(error, retryLabel)` 增加按钮文案参数：用户操作链路保持「重试」（可重发 invoke）；**unhandledrejection 非用户链路改「知道了」**——此类错误无操作可重试，「重试」按钮是误导。

## 三、验证

- 构建消费：`gen/schemas/capabilities.json` 生成 `main-windows`，permissions = `[notification:default, dialog:default]`，windows = `[main, plugin-manager]`。
- 启动冒烟：`cargo test --release --test e2e_startup -- --ignored` **2/2 通过**。
- 发布物重建：`inkosDesktop.app`（capability 编译进二进制）+ `inkosDesktop_0.1.0_aarch64.dmg` 重出（新 sha256 `af1667da…`，CRC 校验过），包内引擎实测可运行（1.8.0）。

## 四、关联提交

- 146 号：fix(src-tauri): notification/dialog 插件 ACL 授权 + 错误横幅文案优化
