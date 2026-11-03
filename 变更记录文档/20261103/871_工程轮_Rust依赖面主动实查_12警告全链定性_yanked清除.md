# 871 号：工程轮——Rust 依赖面主动实查：12 条 audit 警告全链定性 + yanked 清除

日期：2026-11-03。性质：**实质工程轮**（Rust 面；858 号裁决「出口类工项要主动实查外部状态而非静待」的依赖面推广——audit:rust 长期报 12 条 unmaintained/unsound/yanked 警告从未逐链定性）。

## 一、12 条警告全链定性（cargo tree 逐条溯源）

| crate | RUSTSEC | 引入链 | 定性 |
|---|---|---|---|
| unsafe-libyaml 0.2.11 | 2024-0449 unmaintained | **inkos-engine 直接依赖** ← serde_yaml_ng 0.10 | **备案**（见下节） |
| ttf-parser 0.25.1 | 2026-0192 unmaintained | ← lopdf ← pdf-extract ← engine 直接依赖 | 备案：传递依赖，等 pdf-extract 换栈；本地 PDF 导入信任边界内 |
| fxhash 0.2.1 | 2025-0057 | tauri 传递 | 备案：tauri 上游责任 |
| proc-macro-error 1.0.4 | 2024-0370 | tauri build 传递 | 备案：同上 |
| zbus_names 3.0.0/4.3.5 | 2026-0371 | tauri→zbus（Linux DBus） | 备案：zbus 生态 2026 标 unmaintained，Linux-only 传递面，macOS/Windows 构建不触及 |
| zvariant 4.2.0/5.15.1 | 2026-0370 | 同上 | 备案：同上 |
| zvariant_derive 4.2.0/5.15.1 | 2026-0372 | 同上 | 备案：同上 |
| glib 0.18.5 | 2024-0429 **unsound** | tauri→gtk-rs 0.18（Linux） | 备案：unsound 高于 unmaintained 一级，但仍 Linux-only 传递 + VariantStrIter 非引擎数据路径；等 tauri 上游推 gtk 0.20 |
| async-executor 1.14.0 | **yanked** | tauri（target-specific Linux） | **✅ 修复**：1.14.1 已发布，`cargo update --precise` 干净替换 |

## 二、serde_yaml_ng 备案论证（唯一直接依赖项，最高敏感度）

外部实查三连：① crates.io index 直查——serde_yaml_ng **0.9.35~0.10.0 全线钉死 unsafe-libyaml**，无迁移版本；② RUSTSEC-2024-0449 原文——"Patched: no patched versions"，建议替代 = saphyr/serde-saphyr/noyalib；③ 恒等面盘点——runtime_writer `to_string` 对齐 js-yaml `dump(lineWidth:120)` **双栈字节恒等锁** + book_rules/genre_profile/skills 三处 frontmatter 解析语义（对齐 js-yaml+zod）+ golden 向量/duel 锁定。

**裁决：换库代价（破坏恒等锁 + 新生态风险）>> unmaintained 警告（非漏洞、已冻结两年无已知漏洞、本地单机信任边界——解析输入=用户自己项目文件，860 号定性先例）。禁在无恒等证明前更换 YAML 库。** 定性已写进 engine-rs/Cargo.toml serde_yaml_ng 条目注释（防未来轮次误判「警告未清=债未还」而盲换）。

## 三、修复与验证

| 面 | 结果 |
|---|---|
| `cargo update -p async-executor --precise 1.14.1` | ✓ 1.14.0（yanked）→ 1.14.1 |
| audit:rust 复扫 | ✓ src-tauri 警告 **10→9**（yanked 消失）、engine-rs 维持 2、0 漏洞、必红自检活体（7+1 漏洞夹具检出） |
| `cargo check` 双 crate | ✓ dev profile 完成零错误 |
| `clippy:gate` | ✓ 双 crate 0 告警 |
| `cargo:testgate` | ✓ **1984+584 全绿**（47+25 目标） |

## 四、范围备案

- 零生产代码变更（1 行 Cargo.lock + Cargo.toml 注释）——bench 不重跑（853 裁决，基准面零触碰）
- zbus 系/glib/fxhash/proc-macro-error/ttf-parser 的上游动态=新巡检周期实查项（tauri 升级门开启时顺带出清）
- yanked 联网核验超时 328 次=网络抖动非漏洞（846 号勘误先例同形）

**下一号自 872 起；出口不变；用户侧待办=Fork 推送 3 个提交（0069adcc/a4b1f788/本提交）。**
