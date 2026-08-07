//! 插件注册表索引——市场分发的可信来源地基
//!
//! 注册表是**签名 TOML 索引**，列出可安装的插件：元数据 + 下载位置 + 完整性/签名 +
//! 兼容性。两层签名模型：
//!
//! 1. **注册表签名**（detached Ed25519，over `registry.toml` 原始字节）：客户端用受信
//!    公钥（内嵌/配置）验签整个索引。失败则拒绝整个注册表（防篡改条目/注入）。
//!    见 [`PluginRegistryIndex::verify_signature`]。
//! 2. **条目 bundle 签名**（`RegistryEntry::signature`，over 插件包字节）：下载后安装前
//!    验签单个插件包（复用 `sig::verify`）。本模块仅做格式校验；安装期验签属后续 slice。
//!
//! 字段与 [`crate::plugin::manifest::PluginManifest`] 对齐（`capabilities` 用相同字符串
//! 语法），便于下载后生成 manifest 并比对一致性。`download_url` 强制 `https://`（TLS，
//! 防 MITM 下载链）。`version`/`min_host_version` 用 semver 校验（兼容性判断）。
//!
//! v1 约束：每个 `id` 唯一（一条目=最新版）；多版本列表为未来扩展。

use crate::plugin::manifest::parse_capability;
use crate::updater::sig;
use anyhow::{bail, Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 注册表索引格式版本（破坏性 schema 变更时递增；客户端拒绝对应不上者）
pub const REGISTRY_FORMAT_VERSION: u32 = 1;

/// SHA-256 hex 长度（字节 × 2）
const SHA256_HEX_LEN: usize = 64;
/// Ed25519 签名 hex 长度（64 字节 × 2）
const ED25519_SIG_HEX_LEN: usize = 128;

/// 注册表索引（`registry.toml`）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRegistryIndex {
    /// 格式版本（必须 == REGISTRY_FORMAT_VERSION）
    pub version: u32,
    /// 生成时间（ISO 8601，发布方填，仅展示用）
    #[serde(default)]
    pub generated_at: Option<String>,
    /// 插件条目（v1：id 唯一）
    #[serde(default)]
    pub plugins: Vec<RegistryEntry>,
}

/// 单个可安装插件条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub name: String,
    /// 插件版本（SemVer）
    pub version: String,
    pub description: String,
    pub author: String,
    pub homepage: Option<String>,
    pub license: String,
    /// ABI 版本（与宿主兼容性检查）
    pub abi_version: String,
    /// 最低宿主版本（SemVer；客户端版本低于此则不展示/不安装）
    pub min_host_version: String,
    /// 下载 URL（强制 https://）
    pub download_url: String,
    /// 插件包 SHA-256（hex，64 位）——完整性
    pub sha256: String,
    /// 插件包 Ed25519 签名（hex，128 位）——来源认证
    pub signature: String,
    /// 能力声明（与 manifest capability 字符串语法一致）
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// 依赖的其他插件（id → version req）
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

impl PluginRegistryIndex {
    /// 从 TOML 字符串解析注册表索引（含格式版本 + 条目字段全量校验）
    pub fn parse(toml_str: &str) -> Result<Self> {
        let index: PluginRegistryIndex =
            toml::from_str(toml_str).context("注册表 TOML 解析失败")?;
        index.validate()?;
        Ok(index)
    }

    /// 校验：格式版本 + id 唯一 + 每条目字段
    fn validate(&self) -> Result<()> {
        if self.version != REGISTRY_FORMAT_VERSION {
            bail!(
                "不支持的注册表格式版本: {}（当前支持 {}）",
                self.version,
                REGISTRY_FORMAT_VERSION
            );
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for entry in self.plugins.iter() {
            // entry.validate 的消息已含 id（如「插件 X version 非法 semver」），
            // 不再套 with_context——避免 anyhow to_string 只露外层 context、埋掉根因。
            entry.validate()?;
            if !seen.insert(entry.id.as_str()) {
                bail!("注册表含重复插件 id: {}（v1 要求 id 唯一）", entry.id);
            }
        }
        Ok(())
    }

    /// 验证注册表 detached 签名（Ed25519 over 原始 toml 字节）。
    ///
    /// 调用顺序：读原始字节 → `verify_signature(raw, sig_hex, pubkey)` → 通过则
    /// `parse(raw_str)`。先验签再解析，确保只信任受信源签发的内容。
    pub fn verify_signature(raw_toml_bytes: &[u8], sig_hex: &str, pubkey: &[u8]) -> Result<()> {
        let sig_bytes = sig::decode_hex(sig_hex).context("注册表签名 hex 解码失败")?;
        sig::verify(raw_toml_bytes, &sig_bytes, pubkey)
            .context("注册表签名验证失败——拒绝该注册表（可能被篡改）")
    }

    /// 按 id 查找条目
    pub fn find(&self, id: &str) -> Option<&RegistryEntry> {
        self.plugins.iter().find(|e| e.id == id)
    }
}

impl RegistryEntry {
    /// 校验单条目字段（id/version semver/min_host_version semver/url https/
    /// sha256 64hex/signature 128hex/abi_version 非空/capabilities 语法合法）
    fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            bail!("插件 id 不能为空");
        }
        if self.version.is_empty() {
            bail!("插件 {} version 不能为空", self.id);
        }
        Version::parse(&self.version)
            .map_err(|_| anyhow::anyhow!("插件 {} version 非法 semver: {}", self.id, self.version))?;
        Version::parse(&self.min_host_version).map_err(|_| {
            anyhow::anyhow!("插件 {} min_host_version 非法 semver: {}", self.id, self.min_host_version)
        })?;
        if !self.download_url.starts_with("https://") {
            bail!(
                "插件 {} download_url 必须为 https://（强制 TLS）: {}",
                self.id,
                self.download_url
            );
        }
        if !is_hex_of_len(&self.sha256, SHA256_HEX_LEN) {
            bail!("插件 {} sha256 必须为 {} 位 hex: {}", self.id, SHA256_HEX_LEN, self.sha256);
        }
        if !is_hex_of_len(&self.signature, ED25519_SIG_HEX_LEN) {
            bail!(
                "插件 {} signature 必须为 {} 位 hex（Ed25519 64 字节）: {}",
                self.id,
                ED25519_SIG_HEX_LEN,
                self.signature
            );
        }
        if self.abi_version.is_empty() {
            bail!("插件 {} abi_version 不能为空", self.id);
        }
        for cap in &self.capabilities {
            if parse_capability(cap).is_none() {
                bail!("插件 {} 含非法 capability 声明: {}", self.id, cap);
            }
        }
        Ok(())
    }
}

/// 字符串是否为指定长度的 hex
fn is_hex_of_len(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    /// 128 位 hex 占位签名（条目 bundle 签名格式校验用；真实值由发布方签 bundle 产生）
    fn placeholder_sig() -> String {
        "ab".repeat(ED25519_SIG_HEX_LEN / 2)
    }

    /// 构造合法注册表 TOML（可覆盖个别字段造非法用例）
    fn valid_registry_toml() -> String {
        format!(
            r#"
version = 1
generated_at = "2026-08-07T12:00:00Z"

[[plugins]]
id = "example-plugin"
name = "Example"
version = "1.0.0"
description = "An example plugin"
author = "Test Author"
homepage = "https://example.com"
license = "MIT"
abi_version = "1"
min_host_version = "0.1.0"
download_url = "https://example.com/plugin-1.0.0.tar.gz"
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
signature = "{sig}"
capabilities = ["read_project", "network:api.example.com"]

[[plugins]]
id = "another-plugin"
name = "Another"
version = "2.1.3"
description = ""
author = ""
license = "Apache-2.0"
abi_version = "1"
min_host_version = "0.1.0"
download_url = "https://cdn.example.com/another-2.1.3.tar.gz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
signature = "{sig}"
capabilities = []
"#,
            sig = placeholder_sig()
        )
    }

    #[test]
    fn test_parse_valid_registry() {
        let idx = PluginRegistryIndex::parse(&valid_registry_toml()).unwrap();
        assert_eq!(idx.version, REGISTRY_FORMAT_VERSION);
        assert_eq!(idx.plugins.len(), 2);
        assert_eq!(idx.plugins[0].id, "example-plugin");
        assert_eq!(idx.plugins[0].capabilities.len(), 2);
        assert_eq!(idx.generated_at.as_deref(), Some("2026-08-07T12:00:00Z"));
    }

    #[test]
    fn test_parse_rejects_bad_format_version() {
        let toml = valid_registry_toml().replacen("version = 1", "version = 2", 1);
        let err = PluginRegistryIndex::parse(&toml).unwrap_err();
        assert!(err.to_string().contains("不支持的注册表格式版本"));
    }

    #[test]
    fn test_parse_rejects_invalid_toml() {
        assert!(PluginRegistryIndex::parse("not toml [[[").is_err());
    }

    #[test]
    fn test_parse_rejects_entry_bad_version() {
        let toml = valid_registry_toml().replacen("version = \"1.0.0\"", "version = \"not-semver\"", 1);
        let err = PluginRegistryIndex::parse(&toml).unwrap_err();
        assert!(err.to_string().contains("非法 semver"));
    }

    #[test]
    fn test_parse_rejects_entry_http_url() {
        let toml = valid_registry_toml().replacen("https://example.com/plugin", "http://example.com/plugin", 1);
        let err = PluginRegistryIndex::parse(&toml).unwrap_err();
        assert!(err.to_string().contains("必须为 https://"));
    }

    #[test]
    fn test_parse_rejects_entry_bad_sha256() {
        let toml = valid_registry_toml().replacen(
            "sha256 = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"",
            "sha256 = \"tooshort\"",
            1,
        );
        assert!(PluginRegistryIndex::parse(&toml).unwrap_err().to_string().contains("sha256"));
    }

    #[test]
    fn test_parse_rejects_entry_bad_signature() {
        let toml = valid_registry_toml().replacen(
            &format!("signature = \"{}\"", placeholder_sig()),
            "signature = \"deadbeef\"",
            1,
        );
        assert!(PluginRegistryIndex::parse(&toml).unwrap_err().to_string().contains("signature"));
    }

    #[test]
    fn test_parse_rejects_bad_capability() {
        let toml = valid_registry_toml().replacen(
            "\"read_project\", \"network:api.example.com\"",
            "\"read_project\", \"bogus_capability\"",
            1,
        );
        let err = PluginRegistryIndex::parse(&toml).unwrap_err();
        assert!(err.to_string().contains("非法 capability"));
    }

    #[test]
    fn test_parse_rejects_duplicate_id() {
        // 把 another-plugin 的 id 改成与 example-plugin 重复
        let toml = valid_registry_toml().replacen("id = \"another-plugin\"", "id = \"example-plugin\"", 1);
        let err = PluginRegistryIndex::parse(&toml).unwrap_err();
        assert!(err.to_string().contains("重复插件 id"));
    }

    #[test]
    fn test_verify_signature_accepts_valid() {
        let signing = SigningKey::generate(&mut OsRng);
        let raw = valid_registry_toml();
        let sig_hex = sig::encode_hex(&signing.sign(raw.as_bytes()).to_bytes());
        let pubkey = signing.verifying_key().to_bytes();
        PluginRegistryIndex::verify_signature(raw.as_bytes(), &sig_hex, &pubkey).unwrap();
    }

    #[test]
    fn test_verify_signature_rejects_tampered() {
        let signing = SigningKey::generate(&mut OsRng);
        let raw = valid_registry_toml();
        let sig_hex = sig::encode_hex(&signing.sign(raw.as_bytes()).to_bytes());
        let pubkey = signing.verifying_key().to_bytes();
        // 篡改一个字节
        let mut tampered = raw.into_bytes();
        tampered[0] ^= 0xff;
        assert!(PluginRegistryIndex::verify_signature(&tampered, &sig_hex, &pubkey).is_err());
    }

    #[test]
    fn test_verify_signature_rejects_wrong_key() {
        let signing_a = SigningKey::generate(&mut OsRng);
        let signing_b = SigningKey::generate(&mut OsRng);
        let raw = valid_registry_toml();
        let sig_hex = sig::encode_hex(&signing_a.sign(raw.as_bytes()).to_bytes());
        // 用 B 的公钥验 A 的签名 → 失败
        assert!(PluginRegistryIndex::verify_signature(raw.as_bytes(), &sig_hex, &signing_b.verifying_key().to_bytes()).is_err());
    }

    #[test]
    fn test_verify_signature_rejects_bad_hex() {
        let signing = SigningKey::generate(&mut OsRng);
        let raw = valid_registry_toml();
        let pubkey = signing.verifying_key().to_bytes();
        // 非 hex 签名
        assert!(PluginRegistryIndex::verify_signature(raw.as_bytes(), "ZZZZ", &pubkey).is_err());
    }

    #[test]
    fn test_find_by_id() {
        let idx = PluginRegistryIndex::parse(&valid_registry_toml()).unwrap();
        assert_eq!(idx.find("example-plugin").map(|e| e.version.as_str()), Some("1.0.0"));
        assert!(idx.find("nope").is_none());
    }

    #[test]
    fn test_is_hex_of_len() {
        assert!(is_hex_of_len("ab12", 4));
        assert!(!is_hex_of_len("ab12", 5)); // 长度错
        assert!(!is_hex_of_len("xy", 2)); // 非 hex
    }
}
