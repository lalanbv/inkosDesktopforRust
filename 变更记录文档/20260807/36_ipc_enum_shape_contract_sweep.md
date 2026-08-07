# 健壮性：IPC 枚举形态契约全库排查

> 日期：2026-08-07
> 范围：`src/config/types.rs`
> commit：`c29bffba`
> 前置：`35_capability_ui_contract_fix.md`（同类缺陷的首次实证）

## 动机

35 号修复的根因是**跨语言边界的序列化形态漂移**：Rust 侧给枚举变体加字段，
serde 输出从裸字符串变为单键对象，而前端仍做字符串比较 → 静默失效，
无编译错误、无测试失败。

这类缺陷不会只出现一次。系统扫描全库找同构风险点。

## 扫描方法

判据：**同一枚举内混合单元变体与数据变体**。此时 serde 产出两种形态
（`"latest"` 与 `{"fixed":"x"}`），前端必须同时处理；只处理一种即为潜在缺陷。

```
grep -rl "Serialize" --include="*.rs" src | while read f; do
  awk '识别 serde 枚举 → 判断是否同时含 `Variant,` 与 `Variant(`/`Variant {`'
done
```

## 结果：全库仅两处

| 枚举 | 混合形态 | 契约测试 |
|------|---------|---------|
| `Capability` | `"read_project"` / `{"system_command":{...}}` | 35 号新增 |
| `VersionPolicy` | `"latest"` / `{"fixed":"0.4.0"}` | **本轮新增** |

其余 IPC 枚举（`ConfigLayer`、`PluginState`、`SigAction` 等）全为单元变体，
形态恒定为裸字符串，无漂移空间。

## VersionPolicy 现状核查

前端双向逻辑**已正确**处理两种形态：

- `fillConfigForm`：`typeof vp === "object"` → `"fixed" in vp` → `vp.fixed`
- `buildConfigFromForm`：构造 `{fixed:"x"}` / `{range:"^x"}` / `"latest"`

`syncPolicyRows` 的 `p !== "fixed"` 比较的是 `<select>` 的 value（DOM 值，
非 Rust 数据），合法。

所以本轮**无缺陷可修**——加的是防回归的契约测试。

## 为何 round-trip 测试不够

既有 `test_version_policy_serialization` 做 TOML round-trip。反向验证：
把 `Fixed(String)` 改成 `Fixed { version: String }`——

```
test_version_policy_serialization ................ ok      ← 照样通过
test_version_policy_json_shape_matches_ui_contract  FAILED
  left:  {"fixed":{"version":"0.4.0"}}
  right: {"fixed":"0.4.0"}
  形态变更须同步 settings.html 的 fillConfigForm
```

round-trip 只验证「序列化后能反序列化回来」，对形态本身无约束。
UI 侧 `vp.fixed` 会取到对象，版本号静默变 `[object Object]` 或空——
与 `SystemCommand` 完全同构。

## 契约测试内容

1. 单元变体 → 裸字符串（`"latest"`）
2. 元组变体 → 单键对象，且**值为字符串**（UI 直接填 `<input>`，取到对象即坏）
3. **反向**：UI 构造的三种形态均可被 Rust 反序列化（双向契约，防只改写入侧）

失败信息直接点名 `settings.html` 的 `fillConfigForm`，把修复位置写进断言里。

## 验证

- `cargo test`：**550 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：零警告
