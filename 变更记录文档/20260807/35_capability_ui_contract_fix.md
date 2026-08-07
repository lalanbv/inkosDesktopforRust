# 安全：system_command 安装告警静默失效 + 注册表安装补告警

> 日期：2026-08-07
> 范围：`picker/settings.html`、`src/plugin/types.rs`
> commit：`71eb2a21`

## 缺陷：跨语言边界的形态漂移

`SystemCommand` 早先是无字段变体，序列化为裸字符串 `"system_command"`。
本轮加 `allowed_commands` 白名单后变成对象：

```json
{"system_command":{"allowed_commands":["ls","git"]}}
```

但 `settings.html` 的安装告警仍在做字符串比较：

```js
const hasSystemCmd = caps.some((c) => c === "system_command");  // 恒 false
```

**后果**：用户安装可执行任意白名单命令的插件时，不再收到任何权限提示。
Rust 侧的改动是正确的（白名单收窄了权限），却让 UI 侧的知情告警静默归零——
没有编译错误，没有测试失败。

同一函数内 `network` 用的是 `"network" in c`（对象判断，正确），
`system_command` 用字符串比较——两种写法并存正是漂移的征兆。

## 三处修复

### 1. `capabilityTag()` 统一形态判断

serde 对两类变体的形态不同（无字段 → 字符串，带字段 → 单键对象）。
各处各写一遍判断必然漏掉一种，故抽成单点：

```js
function capabilityTag(cap) {
  if (typeof cap === "string") return cap;
  if (cap && typeof cap === "object") return Object.keys(cap)[0] || "";
  return "";
}
```

敏感权限检测（`sensitiveCapWarnings`）与标签渲染（`capabilityLabel`）共用。

### 2. `capabilityLabel()` 修 `[object Object]`

原实现 `${CAP_LABELS[key]}(${cap[key]})` 对 `{path:"/tmp"}` 这种嵌套对象
内插出 `系统命令([object Object])`。加 `capabilityDetail` 按类型分派后：

| capability | 修复前 | 修复后 |
|-----------|-------|-------|
| `system_command` | `系统命令([object Object])` | `系统命令(ls,git)` |
| `network` | `网络([object Object])` | `网络(*)` |
| `filesystem` | `文件系统([object Object])` | `文件系统(/tmp)` |

用户现在能直接看到**被授权的具体命令**，而非无信息量的占位符。

### 3. 注册表安装路径补告警

`cmd_install_from_registry` 成功路径此前**完全没有**敏感权限告警——
而这是更常见的安装路径（插件来自远端）。

注册表插件经 Ed25519 验签，但签名只证明「未被中间人篡改」，
不证明发布者无恶意或未被入侵。与本地目录安装共用 `alertInstalled()`。

## 契约测试：断言完整 JSON，不用 contains

既有 `test_capability_serialization` 用 `json.contains("filesystem")` 断言——
形态从字符串变对象时**照样通过**。正是这个松散度让缺陷溜过。

新增 `test_capability_json_shape_matches_ui_contract` 断言完整 JSON 字符串
+ 单键对象结构（UI 取 `Object.keys()[0]` 的前提）。

### 反向验证

临时把期望值改回旧的字符串形态：

```
FAILED: 带字段变体形态变更须同步 settings.html
  left: "{\"system_command\":{\"allowed_commands\":[\"ls\",\"git\"]}}"
 right: "\"system_command\""
```

失败信息直接指向需同步的文件。这是本轮反复用到的手法（见 `34_`）——
测试通过本身不足以证明它在测东西。

## 前端逻辑验证

picker 是内联脚本的独立 HTML，无 JS 测试设施。用 node 提取函数直接验证
13 项断言全通过（含「旧代码此处失效」这条），确认修复有效；
防回归则由 Rust 侧契约测试承担——真正的失效源头是序列化形态变更。

## 验证

- `cargo test`：**549 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：零警告
