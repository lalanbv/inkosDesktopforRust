# 安全：manifest/registry 展示字段长度与控制字符校验

> 日期：2026-08-07
> 范围：`src/plugin/manifest.rs`、`src/plugin/registry.rs`
> commit：`df58ae11`

## 缺陷

`id` / `entrypoint` / `capabilities` 都有严格校验（白名单字符、路径穿越拒绝、
语法解析），但**展示字段**完全无约束：`name`、`description`、`author`、
`license`、`homepage`。

这些字段来自不可信插件包，且生命周期很长：

| 去向 | 风险 |
|------|------|
| `PluginMetadata` 随 `Arc` clone 进每个 WASM Store | MB 级字符串常驻内存，按插件数放大 |
| 结构化日志（`tracing` 字段） | 刷爆日志文件 |
| 经 IPC 进 UI 插件列表 | 撑坏列表布局 |

更具体的攻击面：`name` 含换行可**伪造结构化日志行**——注入
`evil\nplugin.auto_disabled` 让日志出现假造的健康事件，污染诊断依据。

## 修复：分级长度 + 全控制字符拒绝

`is_safe_display_string(s, max_len)` 单点守卫，manifest 与 registry 共用
（防两侧校验漂移，与本轮 `filter_public_addrs`、`apply_env_allowlist` 同一模式）：

| 字段类 | 上限 | 常量 |
|--------|------|------|
| 展示字段（name/author/license） | 128 B | `MAX_DISPLAY_FIELD_LEN` |
| 描述 | 1024 B | `MAX_DESCRIPTION_LEN` |
| URL（homepage） | 512 B | `MAX_URL_LEN` |

**按字节计长**而非字符数：UTF-8 多字节字符下按字符计长会让实际内存
占用放大 4 倍（`s.chars().count() <= 128` 可对应 512 字节）。
内存与日志的压力来自字节数。

控制字符用 `char::is_control()` 全拒——覆盖 `\n`、`\r`、`\t`、
以及 `\u{7f}` 等不可见字符。正常中文元数据不受影响（已反向断言）。

## 注册表侧同样校验

Ed25519 签名只证明「索引未被中间人篡改」，**不证明发布方未提交超长字段**。
发布方本身可能恶意或被入侵——这是本轮反复出现的信任边界判断
（见 `29_` 的 SSRF、`35_` 的能力告警）。

## 反向验证

临时把守卫改成 `true`：

```
test_is_safe_display_string ......................................... FAILED
test_parse_manifest_rejects_oversized_and_control_char_fields ....... FAILED
```

两个测试都失败——守卫拿掉即被捕获，非环境巧合。

## 测试 +3

- `test_is_safe_display_string`：边界（恰好上限通过 / +1 拒绝）、各类控制字符、
  多字节字符按字节计长
- `test_parse_manifest_rejects_oversized_and_control_char_fields`：manifest 侧
  6 种字段破坏，错误点名对应字段名
- `test_entry_validate_rejects_oversized_and_control_char_fields`：registry 侧同构，
  含「正常中文元数据通过」的反向断言（防校验过严）

registry 侧用直接构造变体条目（`RegistryEntry { name: long, ..base }`）
而非 `Box<dyn Fn>` mutator——字段名与坏值就地对照，且避开 clippy 的
`type_complexity`。

## 验证

- `cargo test`：**554 passed / 0 failed**
- `cargo clippy --all-targets -- -D warnings`：零警告
