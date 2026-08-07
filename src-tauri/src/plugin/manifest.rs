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

/// 插件 id 安全格式白名单（防路径逃逸：id 直接 join 进 plugins_dir，须仅允许
/// 字母数字 / `-` / `_`，长度 ≤64）。pub(crate)：registry 复用同一份校验。
pub(crate) fn is_safe_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
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
}
