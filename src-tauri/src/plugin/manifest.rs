//! 插件清单解析

use super::types::{Capability, PluginError, PluginMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// 插件清单（plugin.toml）
#[derive(Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginInfo,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub homepage: Option<String>,
    pub license: String,
    pub abi_version: String,
    pub entrypoint: String,
}

/// 解析插件清单
pub fn parse_manifest(path: &Path) -> Result<PluginMetadata, PluginError> {
    let manifest_path = path.join("plugin.toml");

    let content = fs::read_to_string(&manifest_path).map_err(|e| {
        PluginError::InvalidManifest(format!("无法读取清单文件: {}", e))
    })?;

    let manifest: PluginManifest = toml::from_str(&content).map_err(|e| {
        PluginError::InvalidManifest(format!("TOML 解析失败: {}", e))
    })?;

    // 验证必需字段
    if !is_safe_plugin_id(&manifest.plugin.id) {
        return Err(PluginError::InvalidManifest(format!(
            "id 含非法字符（仅字母数字/-/_，≤64）: {}",
            manifest.plugin.id
        )));
    }

    // entrypoint 路径安全校验：entrypoint 直接 join 进 plugin_dir，
    // 恶意 `../../../etc/passwd` 可逃逸至插件目录外。
    // 允许子目录（`native/plugin.wasm`）但禁止 ParentDir / 绝对路径 / 前缀。
    if !is_safe_entrypoint(&manifest.plugin.entrypoint) {
        return Err(PluginError::InvalidManifest(format!(
            "entrypoint 含路径逃逸字符（禁 .. / 绝对路径）: {}",
            manifest.plugin.entrypoint
        )));
    }

    if manifest.plugin.version.is_empty() {
        return Err(PluginError::InvalidManifest(
            "version 不能为空".to_string(),
        ));
    }

    // 展示字段长度 + 控制字符校验：manifest 来自不可信插件包，这些字段长期驻留
    // 内存、写日志、经 IPC 进 UI。见 is_safe_display_string 文档。
    for (field, value, max) in [
        ("name", &manifest.plugin.name, MAX_DISPLAY_FIELD_LEN),
        ("author", &manifest.plugin.author, MAX_DISPLAY_FIELD_LEN),
        ("license", &manifest.plugin.license, MAX_DISPLAY_FIELD_LEN),
        ("version", &manifest.plugin.version, MAX_DISPLAY_FIELD_LEN),
        ("description", &manifest.plugin.description, MAX_DESCRIPTION_LEN),
    ] {
        if !is_safe_display_string(value, max) {
            return Err(PluginError::InvalidManifest(format!(
                "{field} 超长（>{max}）或含控制字符"
            )));
        }
    }
    if let Some(homepage) = &manifest.plugin.homepage {
        if !is_safe_display_string(homepage, MAX_URL_LEN) {
            return Err(PluginError::InvalidManifest(format!(
                "homepage 超长（>{MAX_URL_LEN}）或含控制字符"
            )));
        }
    }

    // ABI 兼容性校验：插件 abi_version 须等于宿主 HOST_ABI_VERSION。
    // 接口升级时旧版插件在此处被拒，防止 WIT export 名/类型错位的静默 UB。
    if manifest.plugin.abi_version != HOST_ABI_VERSION {
        return Err(PluginError::InvalidManifest(format!(
            "abi_version 不兼容: 插件={}, 宿主={}",
            manifest.plugin.abi_version, HOST_ABI_VERSION
        )));
    }

    // 解析能力声明
    let capabilities = manifest
        .capabilities
        .iter()
        .filter_map(|cap_str| {
            let result = parse_capability(cap_str);
            if result.is_none() {
                eprintln!("Warning: Failed to parse capability: {}", cap_str);
            }
            result
        })
        .collect();

    Ok(PluginMetadata {
        id: manifest.plugin.id,
        name: manifest.plugin.name,
        description: manifest.plugin.description,
        author: manifest.plugin.author,
        homepage: manifest.plugin.homepage,
        license: manifest.plugin.license,
        version: manifest.plugin.version,
        abi_version: manifest.plugin.abi_version,
        capabilities,
        entrypoint: manifest.plugin.entrypoint,
        dependencies: manifest.dependencies,
        enabled: true,
    })
}

/// 宿主支持的 ABI 版本（插件 manifest 的 `abi_version` 须匹配此值方可加载）。
///
/// 升级宿主 WIT 接口时递增；旧版插件在接口破坏后应被拒绝加载，以防
/// 接口不匹配导致的静默 UB（类型大小/语义偏移、export 名变更等）。
/// `pub`：命令层可将此值暴露给前端，告知可接受的插件 ABI 版本。
pub const HOST_ABI_VERSION: &str = "1";

/// 插件 id 安全格式白名单（防路径逃逸：id 直接 join 进 plugins_dir，须仅允许
/// 字母数字 / `-` / `_`，长度 ≤64）。pub(crate)：registry 复用同一份校验。
pub(crate) fn is_safe_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// 展示类字段（name / author / license）长度上限。
/// 这些字段渲染进插件列表单行，超此长度必然是滥用而非正当元数据。
pub(crate) const MAX_DISPLAY_FIELD_LEN: usize = 128;

/// 描述字段长度上限（比展示字段宽松，允许一小段说明文字）。
pub(crate) const MAX_DESCRIPTION_LEN: usize = 1024;

/// URL 字段（homepage）长度上限。
pub(crate) const MAX_URL_LEN: usize = 512;

/// 展示类字符串安全守卫（长度 + 控制字符）。
///
/// manifest / registry 的 `name`、`description`、`author`、`license` 来自
/// **不可信插件包**，且会长期驻留内存（`PluginMetadata` 随 `Arc` clone 进每个
/// WASM Store）、写入日志、经 IPC 送进 UI。无上限则恶意 manifest 可用 MB 级
/// 字符串放大内存占用、刷爆日志、撑坏插件列表布局。
///
/// 同时拒绝控制字符：
/// - `\0` 截断 C 字符串（与 `is_safe_entrypoint` 一致）
/// - `\n` / `\r` 伪造日志行（结构化日志注入，可伪造 `plugin.auto_disabled` 等事件）
/// - 其余 C0 控制符可注入 ANSI 转义序列，污染终端输出
///
/// 允许空串（`homepage`/`description` 可选留空）；`max_len` 由调用方按字段给出。
pub(crate) fn is_safe_display_string(s: &str, max_len: usize) -> bool {
    s.len() <= max_len && !s.chars().any(|c| c.is_control())
}

/// entrypoint 路径安全守卫（防路径遍历）。
///
/// entrypoint 字段直接 `plugin_dir.join(entrypoint)` 用于定位插件入口文件；
/// 若不校验，恶意 `../../../etc/passwd` 可读取插件目录外任意文件。
///
/// 规则（与 registry `is_safe_relative` 对齐）：
/// - 必须非空且 ≤256 字节
/// - 允许相对子路径（`native/plugin.wasm`、`./plugin.wasm`）
/// - 禁止 `..` 组件（`ParentDir`）——包括折叠后逃逸（`a/../../x`）
/// - 禁止绝对路径（`/etc/x`）及 Windows 前缀（`C:\`）
/// - 不含 NUL 字节
pub(crate) fn is_safe_entrypoint(s: &str) -> bool {
    use std::path::{Component, Path};
    if s.is_empty() || s.len() > 256 || s.contains('\0') {
        return false;
    }
    let path = Path::new(s);
    let mut depth: i32 = 0;
    for comp in path.components() {
        match comp {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}                              // `.` 忽略
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false; // `..` 回到根之上
                }
            }
            Component::RootDir | Component::Prefix(_) => return false, // 绝对 / 跨盘
        }
    }
    depth >= 0
}

/// 系统命令名安全格式（`SystemCommand { allowed_commands }` 白名单条目校验）。
///
/// 仅允许**裸可执行名**（字母数字 / `-` / `_` / `.`），**禁**：
/// - `/`（路径 / 路径遍历，如 `/usr/bin/evil`、`../evil`）
/// - 空格、shell 元字符（`;` `|` `&` `$` `` ` `` `<` `>` `(` `)` `{` `}` 等）
///
/// 防白名单本身成命令注入向量——白名单条目直接进 `Command::new(name)`，若有 `/` 或
/// 元字符，攻击者可借此执行任意路径或注入 shell（虽然 `Command::new` 不经 shell，
/// 但路径遍历仍可达任意二进制）。pub(crate)：parse_capability 复用。
pub(crate) fn is_safe_command_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// 解析能力字符串（pub(crate)：registry 模块复用以校验条目 capability 语法一致）
pub(crate) fn parse_capability(s: &str) -> Option<Capability> {    match s {
        "read_project" => Some(Capability::ReadProject),
        "write_project" => Some(Capability::WriteProject),
        // bare = 空白名单（fail-closed；exec 不可达自 WASM，且「任意命令」违背白名单初衷）
        "system_command" => Some(Capability::SystemCommand {
            allowed_commands: Vec::new(),
        }),
        "environment" => Some(Capability::Environment),
        "database" => Some(Capability::Database),
        "ui" => Some(Capability::Ui),
        // 网络：`network` = 任意域名（向后兼容），`network:a.com,b.com` = 白名单
        "network" => Some(Capability::Network {
            allowed_domains: vec!["*".to_string()],
        }),
        _ if s.starts_with("network:") => {
            let domains: Vec<String> = s
                .strip_prefix("network:")
                .unwrap()
                .split(',')
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty())
                .collect();
            Some(Capability::Network {
                allowed_domains: domains,
            })
        }
        // 系统命令白名单：`system_command:ls,git` = 仅 ls/git。命令名经安全校验，
        // 含非法名（路径/shell 元字符）→ 整个 capability 拒绝（fail-closed）。
        _ if s.starts_with("system_command:") => {
            let commands: Vec<String> = s
                .strip_prefix("system_command:")
                .unwrap()
                .split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            if commands.iter().all(|c| is_safe_command_name(c)) {
                Some(Capability::SystemCommand {
                    allowed_commands: commands,
                })
            } else {
                None
            }
        }
        _ if s.starts_with("filesystem:") => {
            let path = s.strip_prefix("filesystem:").unwrap().to_string();
            // 只接受绝对路径：相对路径在 has_filesystem_capability 中会相对
            // **宿主进程 CWD** 解析（而非插件 work_dir），`filesystem:.` 之类
            // 声明可意外匹配到 work_dir 全量访问，而安装时权限提示看起来无害。
            // fail-closed 拒绝，逼迫声明方写明确的绝对路径。
            if path.is_empty() || !Path::new(&path).is_absolute() {
                return None;
            }
            Some(Capability::Filesystem { path })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_capability() {
        assert_eq!(parse_capability("read_project"), Some(Capability::ReadProject));
        assert_eq!(
            parse_capability("network"),
            Some(Capability::Network {
                allowed_domains: vec!["*".to_string()]
            })
        );
        assert_eq!(
            parse_capability("network:api.github.com,registry.npmjs.org"),
            Some(Capability::Network {
                allowed_domains: vec![
                    "api.github.com".to_string(),
                    "registry.npmjs.org".to_string()
                ]
            })
        );
        assert_eq!(
            parse_capability("filesystem:/tmp"),
            Some(Capability::Filesystem {
                path: "/tmp".to_string()
            })
        );
        // 相对路径 fail-closed：canonicalize 会相对宿主 CWD 解析，
        // `filesystem:.` 在 CWD 为 work_dir 祖先时可绕过为全量访问。
        assert_eq!(parse_capability("filesystem:."), None, "相对路径 `.` 应拒绝");
        assert_eq!(
            parse_capability("filesystem:../../etc"),
            None,
            "相对路径 `..` 应拒绝"
        );
        assert_eq!(
            parse_capability("filesystem:sub/dir"),
            None,
            "无前导斜杠的相对路径应拒绝"
        );
        assert_eq!(parse_capability("filesystem:"), None, "空路径应拒绝");
        // system_command 白名单
        assert_eq!(
            parse_capability("system_command"),
            Some(Capability::SystemCommand {
                allowed_commands: vec![]
            }),
            "bare = 空白名单（fail-closed）"
        );
        assert_eq!(
            parse_capability("system_command:ls,git"),
            Some(Capability::SystemCommand {
                allowed_commands: vec!["ls".to_string(), "git".to_string()]
            })
        );
        // 非法命令名（路径遍历 / shell 元字符）→ 整个 capability 拒绝
        assert_eq!(
            parse_capability("system_command:ls,/usr/bin/evil"),
            None,
            "含路径的命令名必须拒绝"
        );
        assert_eq!(
            parse_capability("system_command:ls;rm -rf"),
            None,
            "含 shell 元字符的命令名必须拒绝"
        );
        assert_eq!(parse_capability("invalid"), None);
    }

    #[test]
    fn test_is_safe_command_name() {
        // 裸可执行名：通过
        assert!(is_safe_command_name("ls"));
        assert!(is_safe_command_name("git"));
        assert!(is_safe_command_name("node-18"));
        assert!(is_safe_command_name("python3"));
        assert!(is_safe_command_name("my.tool"));
        // 路径 / 遍历 / shell 元字符 / 空格：拒
        assert!(!is_safe_command_name("/usr/bin/evil"), "绝对路径拒");
        assert!(!is_safe_command_name("../evil"), "遍历拒");
        assert!(!is_safe_command_name("ls;rm"), "分号拒");
        assert!(!is_safe_command_name("ls|cat"), "管道拒");
        assert!(!is_safe_command_name("ls && cat"), "空格/逻辑符拒");
        assert!(!is_safe_command_name("$HOME/evil"), "变量/路径拒");
        assert!(!is_safe_command_name(""), "空拒");
    }

    #[test]
    fn test_parse_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("test-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        let manifest_content = r#"
capabilities = ["read_project", "network"]

[plugin]
id = "test-plugin"
name = "Test Plugin"
version = "1.0.0"
description = "A test plugin"
author = "Test Author"
license = "MIT"
abi_version = "1"
entrypoint = "plugin.wasm"
"#;

        fs::write(plugin_dir.join("plugin.toml"), manifest_content).unwrap();

        // 先测试 TOML 能否正确解析
        let content = fs::read_to_string(plugin_dir.join("plugin.toml")).unwrap();
        let manifest: PluginManifest = toml::from_str(&content).unwrap();
        println!("Raw manifest capabilities: {:?}", manifest.capabilities);
        assert_eq!(manifest.capabilities.len(), 2, "TOML should have 2 capabilities");

        // 再测试完整解析
        let meta = parse_manifest(&plugin_dir).unwrap();
        println!("Parsed metadata: id={}, capabilities={:?}", meta.id, meta.capabilities);
        assert_eq!(meta.id, "test-plugin");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.capabilities.len(), 2, "Expected 2 capabilities, got {}", meta.capabilities.len());
    }

    #[test]
    fn test_parse_manifest_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = parse_manifest(temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_manifest_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("plugin.toml"), "invalid toml").unwrap();

        let result = parse_manifest(temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_manifest_rejects_unsafe_id() {
        // H1：id 直接 join 进 plugins_dir，必须白名单——挡 "../pwned"、"/etc/x"、
        // "a/b"（路径分隔）、空格、空串等（仅允许字母数字/-/_，≤64）。
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("p");
        fs::create_dir_all(&plugin_dir).unwrap();
        for bad_id in ["../pwned", "/etc/x", "a/b", "bad space", ""] {
            let manifest = format!(
                r#"[plugin]
id = "{bad_id}"
name = "x"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
entrypoint = "plugin.wasm"
"#
            );
            fs::write(plugin_dir.join("plugin.toml"), manifest).unwrap();
            assert!(
                parse_manifest(&plugin_dir).is_err(),
                "id={bad_id:?} 应被白名单拒绝"
            );
        }
        // 合法 id 仍通过
        let manifest = r#"[plugin]
id = "ok-plugin_1"
name = "x"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
entrypoint = "plugin.wasm"
"#;
        fs::write(plugin_dir.join("plugin.toml"), manifest).unwrap();
        assert!(parse_manifest(&plugin_dir).is_ok());
    }

    #[test]
    fn test_is_safe_entrypoint() {
        // 合法值
        assert!(is_safe_entrypoint("plugin.wasm"));
        assert!(is_safe_entrypoint("native/plugin.wasm"));
        assert!(is_safe_entrypoint("./plugin.sh"));
        assert!(is_safe_entrypoint("a/b/c.wasm"));

        // 路径遍历：禁止
        assert!(!is_safe_entrypoint("../evil"));
        assert!(!is_safe_entrypoint("../../etc/passwd"));
        assert!(!is_safe_entrypoint("a/../../etc/passwd"));

        // 绝对路径：禁止
        assert!(!is_safe_entrypoint("/etc/passwd"));
        assert!(!is_safe_entrypoint("/usr/bin/sh"));

        // 空串：禁止
        assert!(!is_safe_entrypoint(""));

        // NUL 字节：禁止
        assert!(!is_safe_entrypoint("plug\0in.wasm"));
    }

    #[test]
    fn test_parse_manifest_rejects_traversal_entrypoint() {
        // entrypoint 路径遍历被 parse_manifest 在解析阶段拦截。
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("my-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        let bad_entrypoints = [
            "../../etc/passwd",
            "../sibling/evil.wasm",
            "/usr/bin/sh",
        ];
        for ep in &bad_entrypoints {
            let manifest = format!(
                r#"[plugin]
id = "my-plugin"
name = "x"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
entrypoint = "{ep}"
"#
            );
            fs::write(plugin_dir.join("plugin.toml"), &manifest).unwrap();
            assert!(
                parse_manifest(&plugin_dir).is_err(),
                "entrypoint={ep:?} 应被拒绝（路径遍历）"
            );
        }

        // 合法 entrypoint 仍通过
        let ok_manifest = r#"[plugin]
id = "my-plugin"
name = "x"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "1"
entrypoint = "native/plugin.wasm"
"#;
        fs::write(plugin_dir.join("plugin.toml"), ok_manifest).unwrap();
        assert!(parse_manifest(&plugin_dir).is_ok(), "合法 entrypoint 应通过");
    }

    #[test]
    fn test_parse_manifest_rejects_incompatible_abi() {
        // ABI 版本不匹配 → parse_manifest 拒绝（防接口错位静默 UB）。
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("abi-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        let bad_abi = r#"[plugin]
id = "abi-plugin"
name = "x"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "99"
entrypoint = "plugin.wasm"
"#;
        fs::write(plugin_dir.join("plugin.toml"), bad_abi).unwrap();
        let err = parse_manifest(&plugin_dir);
        assert!(err.is_err(), "abi_version=99 应被拒绝（不兼容宿主）");
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("abi_version"),
            "错误信息应包含 'abi_version'，实际: {msg}"
        );

        // 合法 abi_version 仍通过
        let ok_abi = format!(r#"[plugin]
id = "abi-plugin"
name = "x"
version = "1.0.0"
description = ""
author = ""
license = "MIT"
abi_version = "{HOST_ABI_VERSION}"
entrypoint = "plugin.wasm"
"#);
        fs::write(plugin_dir.join("plugin.toml"), &ok_abi).unwrap();
        assert!(parse_manifest(&plugin_dir).is_ok(), "abi_version={HOST_ABI_VERSION} 应通过");
    }

    #[test]
    fn test_is_safe_display_string() {
        // 长度边界：恰好等于上限通过，超一字节拒绝
        assert!(is_safe_display_string(&"a".repeat(128), 128), "恰好上限应通过");
        assert!(!is_safe_display_string(&"a".repeat(129), 128), "超上限应拒绝");
        assert!(is_safe_display_string("", 128), "空串应通过（可选字段留空）");

        // 控制字符全类别拒绝
        for (label, s) in [
            ("NUL", "ab\0cd"),
            ("换行（伪造日志行）", "ab\ncd"),
            ("回车", "ab\rcd"),
            ("制表", "ab\tcd"),
            ("ANSI 转义", "ab\x1b[31mcd"),
            ("退格", "ab\x08cd"),
        ] {
            assert!(
                !is_safe_display_string(s, 128),
                "{label} 应被拒绝: {s:?}"
            );
        }

        // 正常多字节文本通过（中文插件名/描述是正当用法）
        assert!(is_safe_display_string("我的插件 · Markdown 格式化", 128));
        // 多字节按**字节**计长（防 UTF-8 放大绕过：128 个中文 = 384 字节）
        assert!(
            !is_safe_display_string(&"字".repeat(50), 128),
            "50 个中文 = 150 字节，超 128 上限应拒绝"
        );
    }

    #[test]
    fn test_parse_manifest_rejects_oversized_and_control_char_fields() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("disp-plugin");
        fs::create_dir_all(&plugin_dir).unwrap();

        // 每个字段单独超限/含控制字符 → 拒绝，且错误信息点名该字段
        let long = "A".repeat(MAX_DISPLAY_FIELD_LEN + 1);
        let long_desc = "B".repeat(MAX_DESCRIPTION_LEN + 1);
        let cases: [(&str, String, &str); 5] = [
            ("name", long.clone(), "name"),
            ("author", long.clone(), "author"),
            ("license", long.clone(), "license"),
            ("description", long_desc, "description"),
            // 换行可伪造结构化日志行（如假造 plugin.auto_disabled 事件）
            ("name", "evil\nplugin.auto_disabled".to_string(), "name"),
        ];

        for (field, value, expect_in_msg) in cases {
            // TOML 基础字段，逐个用 value 覆盖目标字段
            let mut fields = std::collections::HashMap::from([
                ("name", "x".to_string()),
                ("description", String::new()),
                ("author", String::new()),
                ("license", "MIT".to_string()),
            ]);
            fields.insert(field, value.clone());
            let manifest = format!(
                r#"[plugin]
id = "disp-plugin"
name = {}
version = "1.0.0"
description = {}
author = {}
license = {}
abi_version = "{HOST_ABI_VERSION}"
entrypoint = "plugin.wasm"
"#,
                toml_str(&fields["name"]),
                toml_str(&fields["description"]),
                toml_str(&fields["author"]),
                toml_str(&fields["license"]),
            );
            fs::write(plugin_dir.join("plugin.toml"), &manifest).unwrap();
            let err = parse_manifest(&plugin_dir)
                .expect_err(&format!("{field} 超长/含控制字符应被拒绝"));
            let msg = err.to_string();
            assert!(
                msg.contains(expect_in_msg),
                "错误应点名字段 {expect_in_msg}，实际: {msg}"
            );
        }

        // 反向：正常长度的中文字段仍通过（不过严）
        let ok = format!(
            r#"[plugin]
id = "disp-plugin"
name = "我的插件"
version = "1.0.0"
description = "一个用于格式化 Markdown 的插件"
author = "张三"
license = "MIT"
abi_version = "{HOST_ABI_VERSION}"
entrypoint = "plugin.wasm"
"#
        );
        fs::write(plugin_dir.join("plugin.toml"), &ok).unwrap();
        assert!(
            parse_manifest(&plugin_dir).is_ok(),
            "正常中文元数据应通过（校验不应过严）"
        );
    }

    /// 把字符串包成 TOML 基本字符串字面量（转义反斜杠/引号/换行，
    /// 否则含 `\n` 的测试值会破坏 TOML 语法，测试失败原因变成解析错误）。
    fn toml_str(s: &str) -> String {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r");
        format!("\"{escaped}\"")
    }
}
