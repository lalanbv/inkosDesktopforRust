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
use flate2::read::GzDecoder;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};
use tar::Archive;

// 复用 updater::sig 的 hex 解码，re-export 供命令层解码配置中的 pubkey（封装：命令
// 仅依赖 plugin::registry API，不直接耦合 updater::sig）。
pub use crate::updater::sig::decode_hex;

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
    /// 校验单条目字段（id 白名单/version semver/min_host_version semver/url https/
    /// sha256 64hex/signature 128hex/abi_version 非空/capabilities 语法合法）。
    /// pub：命令层 `cmd_install_from_registry` 收到前端传入 entry 后须重新校验。
    pub fn validate(&self) -> Result<()> {
        if !crate::plugin::manifest::is_safe_plugin_id(&self.id) {
            bail!("插件 id 含非法字符（仅字母数字/-/_，≤64）: {}", self.id);
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

    /// 是否与当前宿主兼容（`min_host_version` ≤ host_version 且 abi_version 精确匹配）。
    ///
    /// v1 用 abi 精确匹配（host 与插件 ABI 同为 "1"）；未来若引入 ABI 向后兼容矩阵，
    /// 扩展为 `host_supported_abis.contains(&self.abi_version)`。
    pub fn is_compatible_with(&self, host_version: &Version, host_abi: &str) -> bool {
        let Ok(entry_min) = Version::parse(&self.min_host_version) else {
            return false; // parse 已校验过，此处仅防御
        };
        host_version >= &entry_min && self.abi_version == host_abi
    }

    /// 校验下载的插件包字节：sha256 完整性 + Ed25519 来源签名。
    ///
    /// `pubkey` 为受信公钥——当前实现使用**配置的 registry 公钥**（单 key 模型：
    /// 同一密钥签注册表与插件包，见 `cmd_install_from_registry`）。未来若要分离
    /// （注册表 key 与插件 key 独立，避免单 key 泄露同时崩两边），扩展
    /// `PluginRegistryConfig` 加 `plugin_pubkey`，此处改用之。
    /// 下载后、安装前调用——任一失败拒绝该包。
    pub fn verify_bundle(&self, bundle: &[u8], pubkey: &[u8]) -> Result<()> {
        let hash = Sha256::digest(bundle);
        let hash_hex = sig::encode_hex(&hash);
        if hash_hex != self.sha256 {
            bail!(
                "插件 {} 包 sha256 不匹配：期望 {}，实际 {}（包可能损坏/被篡改）",
                self.id,
                self.sha256,
                hash_hex
            );
        }
        let sig_bytes = sig::decode_hex(&self.signature).context("插件签名 hex 解码失败")?;
        sig::verify(bundle, &sig_bytes, pubkey).with_context(|| {
            format!("插件 {} 包签名验证失败——拒绝（可能被篡改）", self.id)
        })?;
        Ok(())
    }
}

/// 字符串是否为指定长度的 hex
fn is_hex_of_len(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 拉取并验签注册表索引。
///
/// `fetch` 注入 HTTP 传输（`Fn(&str) -> Future<Output = Result<Vec<u8>>>`）——
/// 生产用 [`http_fetch`]（reqwest），测试用桩，使「验签 + 解析」逻辑脱离网络单测。
/// 流程：拉 `registry_url` 字节 + `{url}.sig` hex → `verify_signature` → `parse`。
///
/// 注册表 URL 不强制 https：注册表本身经 Ed25519 签名，即便经 http 也无法被无私钥
/// 的 MITM 伪造合法签名（签名是信任锚，https 仅纵深防御）。插件包 `download_url`
/// 的 https 强制在条目校验中保留（包虽也签名，https 是安装链的额外保障）。
pub async fn fetch_registry<F, Fut>(
    registry_url: &str,
    pubkey: &[u8],
    fetch: F,
) -> Result<PluginRegistryIndex>
where
    F: Fn(&str) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>>>,
{
    let toml_bytes = fetch(registry_url).await.context("拉取 registry.toml 失败")?;
    let sig_bytes = fetch(&format!("{registry_url}.sig"))
        .await
        .context("拉取 registry.toml.sig 失败")?;
    let sig_hex = String::from_utf8(sig_bytes)
        .context("registry 签名非合法 UTF-8 hex")?
        .trim()
        .to_string();
    PluginRegistryIndex::verify_signature(&toml_bytes, &sig_hex, pubkey)?;
    let toml_str = String::from_utf8(toml_bytes).context("registry.toml 非合法 UTF-8")?;
    PluginRegistryIndex::parse(&toml_str)
}

/// 注册表索引（registry.toml + .sig）最大字节数——防放大攻击 OOM
pub const MAX_REGISTRY_BYTES: u64 = 8 * 1024 * 1024;
/// 插件包（bundle）最大字节数——防超大下载 OOM
pub const MAX_BUNDLE_BYTES: u64 = 64 * 1024 * 1024;

/// 生产 HTTP 传输：reqwest GET → bytes，带 `max_bytes` 流式上限（防放大/OOM）。
///
/// Content-Length 预检（可伪造但挡多数放大）+ 流式累加超限中止（防伪造
/// Content-Length 的真实放大）。
pub async fn http_fetch(client: &reqwest::Client, url: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let mut resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url} 失败"))?;
    if !resp.status().is_success() {
        bail!("GET {url} 返回非成功状态: {}", resp.status());
    }
    if let Some(len) = resp.content_length() {
        if len > max_bytes {
            bail!("GET {url} 响应过大（Content-Length {len} > 上限 {max_bytes}）");
        }
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .with_context(|| format!("读 {url} 响应体失败"))?
    {
        if buf.len() + chunk.len() > max_bytes as usize {
            bail!("GET {url} 响应超过上限 {max_bytes} 字节（防 OOM）");
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// 安全解压 tar.gz 到 `dest`——拒绝路径逃逸条目（防 tar-slip：`../` 越界、绝对路径、
/// 跨盘前缀）。注册表 bundle 须为 tar.gz，**条目置于根**（`plugin.toml` + `.wasm`，
/// 无外层目录），解压后 `dest` 即可安装。
///
/// 安全关键：归档字节虽经 `verify_bundle` 验签，仍防御性校验路径——签名防伪造，路径
/// 校验防合法发布方的工具链意外产出越界条目，也防验签密钥未来泄露后的恶意归档。
pub fn safe_extract_tar_gz(archive: &[u8], dest: &Path) -> Result<()> {
    let gz = GzDecoder::new(archive);
    let mut tar = Archive::new(gz);
    std::fs::create_dir_all(dest)
        .with_context(|| format!("创建解压目录失败: {}", dest.display()))?;

    for entry in tar.entries().context("枚举 tar 条目失败")? {
        let mut entry = entry.context("读 tar 条目失败")?;
        let raw_path = entry.path().context("读条目路径失败")?.into_owned();
        if !is_safe_relative(&raw_path) {
            bail!("tar 含路径逃逸条目，拒绝解压: {}", raw_path.display());
        }
        // 关闭权限位保留（跨平台一致；避免解压出 setuid 等危险位）
        entry.set_preserve_permissions(false);
        entry
            .unpack_in(dest)
            .with_context(|| format!("解压条目失败: {}", raw_path.display()))?;
    }
    Ok(())
}

/// 路径是否为安全相对路径：仅 Normal/CurDir/ParentDir，且 `..` 折叠不回到根之上
/// （depth 不为负）；拒 RootDir（绝对）/Prefix（跨盘）。
fn is_safe_relative(path: &Path) -> bool {
    let mut depth: i32 = 0;
    for comp in path.components() {
        match comp {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {} // `.` 忽略
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

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use rand::rngs::OsRng;
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::io::Write;
    use tar::{Builder, Header};
    use tempfile::TempDir;

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

    /// 用签名钥对 bundle 签名 + 算 sha256，构造合法条目（verify_bundle 测试用）
    fn entry_from_bundle(id: &str, bundle: &[u8], signing: &SigningKey) -> RegistryEntry {
        let hash = Sha256::digest(bundle);
        let sig_bytes = signing.sign(bundle);
        RegistryEntry {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            min_host_version: "0.1.0".to_string(),
            download_url: "https://example.com/p.tar.gz".to_string(),
            sha256: sig::encode_hex(&hash),
            signature: sig::encode_hex(&sig_bytes.to_bytes()),
            capabilities: Vec::new(),
            dependencies: HashMap::new(),
        }
    }

    #[test]
    fn test_is_compatible_with() {
        let entry = entry_from_bundle("p", b"bundle", &SigningKey::generate(&mut OsRng));
        let host_0_1 = Version::parse("0.1.0").unwrap();
        let host_0_0_5 = Version::parse("0.0.5").unwrap();
        assert!(entry.is_compatible_with(&host_0_1, "1")); // 满足 min + abi
        assert!(!entry.is_compatible_with(&host_0_0_5, "1")); // host 低于 min_host_version
        assert!(!entry.is_compatible_with(&host_0_1, "2")); // abi 不匹配
    }

    #[test]
    fn test_verify_bundle_accepts_valid() {
        let signing = SigningKey::generate(&mut OsRng);
        let bundle = b"plugin bundle bytes";
        let entry = entry_from_bundle("p", bundle, &signing);
        entry
            .verify_bundle(bundle, &signing.verifying_key().to_bytes())
            .unwrap();
    }

    #[test]
    fn test_verify_bundle_rejects_tampered() {
        let signing = SigningKey::generate(&mut OsRng);
        let bundle = b"plugin bundle bytes";
        let entry = entry_from_bundle("p", bundle, &signing);
        let mut tampered = bundle.to_vec();
        tampered[0] ^= 0xff; // 篡改 → sha256 不匹配（签名也失效）
        assert!(entry
            .verify_bundle(&tampered, &signing.verifying_key().to_bytes())
            .is_err());
    }

    #[test]
    fn test_verify_bundle_rejects_wrong_pubkey() {
        let signing_a = SigningKey::generate(&mut OsRng);
        let signing_b = SigningKey::generate(&mut OsRng);
        let bundle = b"plugin bundle bytes";
        let entry = entry_from_bundle("p", bundle, &signing_a);
        // 用 B 的公钥验 A 签的包 → 签名失败
        assert!(entry
            .verify_bundle(bundle, &signing_b.verifying_key().to_bytes())
            .is_err());
    }

    /// 构造 mock fetch 传输：`.sig` URL 返回 sig_hex 字节，其余返回 toml 字节。
    /// 每次 clone，保证闭包可多次调用。
    fn registry_fetch(
        toml_bytes: Vec<u8>,
        sig_hex: String,
    ) -> impl Fn(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>>>>> {
        move |url: &str| {
            let payload = if url.ends_with(".sig") {
                sig_hex.clone().into_bytes()
            } else {
                toml_bytes.clone()
            };
            Box::pin(async move { Ok(payload) })
        }
    }

    #[tokio::test]
    async fn test_fetch_registry_accepts_valid() {
        let signing = SigningKey::generate(&mut OsRng);
        let toml = valid_registry_toml();
        let sig_hex = sig::encode_hex(&signing.sign(toml.as_bytes()).to_bytes());
        let pubkey = signing.verifying_key().to_bytes();
        let index = fetch_registry(
            "http://127.0.0.1/registry.toml",
            &pubkey,
            registry_fetch(toml.into_bytes(), sig_hex),
        )
        .await
        .unwrap();
        assert_eq!(index.plugins.len(), 2);
    }

    #[tokio::test]
    async fn test_fetch_registry_rejects_bad_signature() {
        let signing = SigningKey::generate(&mut OsRng);
        let toml = valid_registry_toml();
        // 用「别的数据」的签名 → verify_signature 失败
        let sig_hex = sig::encode_hex(&signing.sign(b"totally different bytes").to_bytes());
        let pubkey = signing.verifying_key().to_bytes();
        let result = fetch_registry(
            "http://127.0.0.1/registry.toml",
            &pubkey,
            registry_fetch(toml.into_bytes(), sig_hex),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_registry_rejects_bad_toml() {
        let signing = SigningKey::generate(&mut OsRng);
        // 非法 TOML——但用正确密钥对它签名（验签过、parse 挂）
        let bad_toml = "invalid toml [[[";
        let sig_hex = sig::encode_hex(&signing.sign(bad_toml.as_bytes()).to_bytes());
        let pubkey = signing.verifying_key().to_bytes();
        let result = fetch_registry(
            "http://127.0.0.1/registry.toml",
            &pubkey,
            registry_fetch(bad_toml.as_bytes().to_vec(), sig_hex),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_fetch_registry_rejects_fetch_error() {
        let signing = SigningKey::generate(&mut OsRng);
        let pubkey = signing.verifying_key().to_bytes();
        let fetch_err = |_url: &str|
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<u8>>>>> {
            Box::pin(async { Err(anyhow::anyhow!("网络不可达")) })
        };
        let result = fetch_registry("http://127.0.0.1/registry.toml", &pubkey, fetch_err).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("拉取 registry.toml 失败"));
    }

    /// 构造 tar.gz（entries: (name, content)）——安全解压测试用
    fn build_tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_buf = Vec::new();
        {
            let mut builder = Builder::new(&mut tar_buf);
            for (name, data) in entries {
                let mut header = Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, *name, std::io::Cursor::new(*data))
                    .unwrap();
            }
            builder.finish().unwrap();
        }
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&tar_buf).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn test_is_safe_relative_logic() {
        assert!(is_safe_relative(Path::new("plugin.toml")));
        assert!(is_safe_relative(Path::new("a/b/plugin.wasm")));
        assert!(is_safe_relative(Path::new("./x"))); // CurDir
        assert!(!is_safe_relative(Path::new("../x"))); // 回到根之上
        assert!(!is_safe_relative(Path::new("a/../../x"))); // 折叠后逃逸
        assert!(!is_safe_relative(Path::new("/etc/x"))); // 绝对路径
    }

    #[test]
    fn test_safe_extract_valid() {
        let archive = build_tar_gz(&[("plugin.toml", b"id=\"x\""), ("plugin.wasm", b"WASM")]);
        let dest = TempDir::new().unwrap();
        safe_extract_tar_gz(&archive, dest.path()).unwrap();
        assert!(dest.path().join("plugin.toml").exists());
        assert!(dest.path().join("plugin.wasm").exists());
    }

    // 路径逃逸拒绝说明：解压侧守卫是 `is_safe_relative`（上方 test_is_safe_relative_logic
    // 已钉死），`safe_extract` 在每个条目 `unpack_in` 前调用它。Rust `tar` Builder 额外
    // 在**创建侧**拒绝 `..`/绝对路径（"paths in archives must not have `..`"），故无法
    // 用它构造攻击归档来跑解压侧的端到端拒绝测试——这本身就是一层防护。真实场景中
    // 其他工具产出的恶意归档由 `is_safe_relative` 在解压侧拦截。
}
