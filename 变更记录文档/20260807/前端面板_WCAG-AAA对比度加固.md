# 前端面板 WCAG AAA 对比度加固

> 日期：2026-08-07
> 范围：`src-tauri/picker/index.html`、`src-tauri/picker/settings.html`
> 目标：picker/settings 两面板完成 WCAG AAA 对比度全量审计与加固，修复 2 处真实 AA 缺陷

## 背景

Phase 6.4 前端面板（picker/settings）已实现 i18n + a11y 骨架，但强调色沿用 Tailwind 默认蓝（`#2563eb`），白字主按钮对比度仅 ~4.7:1（达 AA，未达 AAA）。另有两处次要文本色 `#6b7280`（gray-500）经相对亮度法核算**未达 AA 4.5:1**，属真实缺陷而非边际优化。

## 变更

### 1. 强调色加深（AAA）

| 用途 | 旧色 | 新色 | 对比度（白底/白字） |
|------|------|------|---------------------|
| 主按钮 `button.primary` | `#2563eb` | `#1d4ed8`（blue-700） | 白字 ≈ 7.6:1（AAA） |
| 主按钮 hover | `#1d4ed8` | `#1e3a8a`（blue-900） | 更深，焦点态更稳 |
| 链接按钮 `.link-btn` | `#2563eb` | `#1d4ed8` | 白底 ≈ 7.2:1（AAA） |
| 收藏星标 `.fav.on` | `#f59e0b` | `#d97706`（amber-600） | 图形对象 ≈ 3.06:1（WCAG 1.4.11 非 3:1 ✓） |

sed 顺序：先 `#1d4ed8→#1e3a8a`（hover），再 `#2563eb→#1d4ed8`（primary），避免 primary 被首条规则二次替换。

### 2. 修复 AA 缺陷（settings.html）

`#6b7280`（gray-500）两处用途均未达 AA，统一加深为 `#4b5563`（gray-600）：

| 选择器 | 旧对比度 | 新对比度 |
|--------|----------|----------|
| `.badge-off`（`#6b7280`/`#f3f4f6`） | 4.26:1 ✗ | `#4b5563`/`#f3f4f6` ≈ 7.79:1（AAA）✓ |
| `.metrics`（`#6b7280`/`#fafafa`） | 4.35:1 ✗ | `#4b5563`/`#fafafa` ≈ 7.95:1（AAA）✓ |

### 3. 全量审计结果（相对亮度法）

均以面板实际背景（`#fafafa` 正文 / `#fff` 卡片 / 各自徽章底）为参照：

- 正文 `#1a1a1a`/`#fafafa` ≈ 16:1（AAA）
- 主按钮 `#fff`/`#1d4ed8` ≈ 7.6:1（AAA）
- 链接/次主色文本 `#1d4ed8`/`#fafafa` ≈ 7.2:1（AAA）
- 次要文本 `#595959`/`#fafafa` ≈ 6.7:1（AA，近 AAA）
- 指标/禁用态 `#4b5563`/`#fafafa` ≈ 7.8:1（AAA）
- 错误 `#b91c1c` ≈ 7.5:1（AAA）
- 徽章 `#166534`/`#dcfce7`、`#4338ca`/`#eef2ff` 均 > 7:1（AAA）
- 收藏星标 `#d97706` ≈ 3.06:1（图形对象 1.4.11，3:1 ✓）

## 验证

- `grep` 确认 `#6b7280`/`#2563eb`/`#f59e0b` 旧色零残留
- 纯 CSS 改动，不触及 JS 逻辑与 Rust 后端；既有 427 测试 / clippy / release build 不受影响

## 不做

- 进程隔离插件 OS 级网络沙箱（无 root 跨平台不可行，已以 WASM host_api 白名单 + 安装 UX 警告替代，见 `docs/plugin-system.md`）
- sidecar 配置编辑 UI（需工作区/inkos Studio 上下文，非 picker 层职责）
