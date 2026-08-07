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

/// 解析能力字符串（pub(crate)：registry 模块复用以校验条目 capability 语法一致）
pub(crate) fn parse_capability(s: &str) -> Option<Capability> {    match s {
        "read_project" => Some(Capability::ReadProject),
        "write_project" => Some(Capability::WriteProject),
        "system_command" => Some(Capability::SystemCommand),
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
        assert_eq!(parse_capability("invalid"), None);
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
}
