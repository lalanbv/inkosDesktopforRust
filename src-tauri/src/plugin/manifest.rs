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
    if manifest.plugin.id.is_empty() {
        return Err(PluginError::InvalidManifest("id 不能为空".to_string()));
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

/// 解析能力字符串
fn parse_capability(s: &str) -> Option<Capability> {
    match s {
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
}
