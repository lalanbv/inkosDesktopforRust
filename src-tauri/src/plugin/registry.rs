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

use crate::plugin::host_api::{extract_host, is_internal_ip};
use crate::plugin::manifest::parse_capability;
use crate::updater::sig;
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

    /// 校验：格式版本 + 每条目字段（多版本：同 id 允许多个条目，不同版本）
    fn validate(&self) -> Result<()> {
        if self.version != REGISTRY_FORMAT_VERSION {
            bail!(
                "不支持的注册表格式版本: {}（当前支持 {}）",
                self.version,
                REGISTRY_FORMAT_VERSION
            );
        }
        for entry in self.plugins.iter() {
            // entry.validate 的消息已含 id（如「插件 X version 非法 semver」），
            // 不再套 with_context——避免 anyhow to_string 只露外层 context、埋掉根因。
            entry.validate()?;
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

    /// 按 id 查找**最高版本**条目（多版本注册表：同 id 可有多条，取 semver 最高）。
    pub fn find_latest(&self, id: &str) -> Option<&RegistryEntry> {
        self.plugins
            .iter()
            .filter(|e| e.id == id)
            .filter_map(|e| Version::parse(&e.version).ok().map(|v| (e, v)))
            .max_by(|a, b| a.1.cmp(&b.1))
            .map(|(e, _)| e)
    }

    /// 按 id + 精确版本查找（锁定/回滚指定版本用）。
    pub fn find_version(&self, id: &str, version: &Version) -> Option<&RegistryEntry> {
        self.plugins
            .iter()
            .find(|e| e.id == id && Version::parse(&e.version).ok().as_ref() == Some(version))
    }

    /// 列出某 id 的全部版本条目，按 semver 降序（最高在前）。供版本选择器 UI。
    pub fn find_all_versions(&self, id: &str) -> Vec<&RegistryEntry> {
        let mut versions: Vec<(&RegistryEntry, Version)> = self
            .plugins
            .iter()
            .filter(|e| e.id == id)
            .filter_map(|e| Version::parse(&e.version).ok().map(|v| (e, v)))
            .collect();
        versions.sort_by(|a, b| b.1.cmp(&a.1)); // 降序
        versions.into_iter().map(|(e, _)| e).collect()
    }

    /// 返回每个 id 下**最高兼容版本**的条目（去重 + 兼容过滤）。
    /// 供 browse：一个插件只展示其最新可用版本。版本排序后 id 字典序，UI 稳定。
    pub fn latest_compatible(
        &self,
        host_version: &Version,
        host_abi: &str,
    ) -> Vec<&RegistryEntry> {
        let mut ids: Vec<&str> = self.plugins.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| {
                self.plugins
                    .iter()
                    .filter(|e| e.id == id && e.is_compatible_with(host_version, host_abi))
                    .filter_map(|e| Version::parse(&e.version).ok().map(|v| (e, v)))
                    .max_by(|a, b| a.1.cmp(&b.1))
                    .map(|(e, _)| e)
            })
            .collect()
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

/// 插件更新信息（installed 版本 < registry 可用版本）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub id: String,
    pub installed_version: String,
    pub available_version: String,
}

/// 比较已装插件与注册表，返回有更新（registry 最高版本 > installed）的列表。
///
/// 纯函数：注册表支持多版本（同 id 多条），`find_latest` 取最高版本做 semver 比较。
/// 版本不可解析的条目跳过（不 panic）。
pub fn check_updates(installed: &[(String, String)], registry: &PluginRegistryIndex) -> Vec<UpdateInfo> {
    installed
        .iter()
        .filter_map(|(id, cur)| {
            let entry = registry.find_latest(id)?;
            let Ok(avail) = Version::parse(&entry.version) else {
                return None;
            };
            let Ok(cur_v) = Version::parse(cur) else {
                return None;
            };
            if avail > cur_v {
                Some(UpdateInfo {
                    id: id.clone(),
                    installed_version: cur.clone(),
                    available_version: entry.version.clone(),
                })
            } else {
                None
            }
        })
        .collect()
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
    // scheme 校验：仅允许 https://，拒绝 http://（防明文 MitM）和其他 scheme。
    // 注册表索引 + bundle 均须经 TLS 传输；bundle 有 Ed25519 签名（完整性保障），
    // 但明文传输仍暴露用户隐私（哪些插件被安装）和流量指纹。
    if !url.starts_with("https://") {
        bail!("http_fetch: 仅允许 https:// URL，拒绝: {url}");
    }

    // SSRF 防护：拒绝内网/保留 IP 字面量（无论注册表签名是否通过，下载 URL 均须
    // 经此检查——签名只证明注册表未被中间人篡改，不证明发布者本身无恶意/被入侵）。
    // 涵盖: loopback / 私网 / 链路本地 / 未指定 / 云元数据 (169.254.169.254) /
    // IPv6 ULA + mapped 私网。DNS rebinding 由系统 resolver 解析后 connect 时校验
    // （reqwest 使用 OS resolver，无 IP pinning；签名验证已覆盖条目完整性，此层
    // 仅防 IP 字面量形式的直连内网）。
    if let Some(host) = extract_host(url) {
        if is_internal_ip(&host) {
            bail!("http_fetch: 拒绝内网/保留 IP 字面量 SSRF: {url}（host={host}）");
        }
    }

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

/// 解压后累计字节上限（防 tar-bomb / zip-bomb DoS）。
///
/// 下载侧 `MAX_BUNDLE_BYTES` 仅限**压缩**字节；gzip 对高度重复数据压缩比可达
/// 1000×+，64 MiB 压缩可解压为数十 GB。此上限约束**解压后累计声明大小**，是
/// 「签了但恶意/畸形」威胁模型（密钥泄露 / 发布方工具链意外）下的 DoS 防线。
const MAX_EXTRACTED_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB

/// tar 条目数上限（防海量小条目耗尽 inode / 拖慢解压的 DoS）。
const MAX_TAR_ENTRIES: usize = 4_096;

/// 安全解压 tar.gz 到 `dest`，逐条目防御性校验，拒绝：
/// - **路径逃逸**（`../` 越界、绝对路径、跨盘前缀）—— `is_safe_relative`
/// - **symlink / hardlink 条目**——防落地恶意符号链接被后续 `write_file` 经字面校验
///   利用（闭环 security-audit R1）。tar 0.4.46 的 `validate_inside_dst` 仅防**解压期**
///   经链接逃逸，不阻止链接**落地**，故须在此显式拒。
/// - **累计解压字节 / 条目数超限**——防 tar-bomb DoS
///
/// 注册表 bundle 须为 tar.gz，**条目置于根**（`plugin.toml` + `.wasm`，无外层目录）。
///
/// 安全关键：归档字节虽经 `verify_bundle` 验签，仍防御性校验——签名防伪造，结构校验
/// 防合法发布方工具链意外产出越界/恶意条目，也防验签密钥未来泄露后的恶意归档。
pub fn safe_extract_tar_gz(archive: &[u8], dest: &Path) -> Result<()> {
    let gz = GzDecoder::new(archive);
    let mut tar = Archive::new(gz);
    std::fs::create_dir_all(dest)
        .with_context(|| format!("创建解压目录失败: {}", dest.display()))?;

    let mut total_bytes: u64 = 0;
    let mut entry_count: usize = 0;
    for entry in tar.entries().context("枚举 tar 条目失败")? {
        entry_count += 1;
        if entry_count > MAX_TAR_ENTRIES {
            bail!("tar 条目数超上限 {MAX_TAR_ENTRIES}，拒绝解压（防 DoS）");
        }
        let mut entry = entry.context("读 tar 条目失败")?;
        let raw_path = entry.path().context("读条目路径失败")?.into_owned();
        if !is_safe_relative(&raw_path) {
            bail!("tar 含路径逃逸条目，拒绝解压: {}", raw_path.display());
        }
        let kind = entry.header().entry_type();
        if kind.is_hard_link() || kind.is_symlink() {
            bail!(
                "插件 bundle 不允许 symlink/hardlink 条目（防符号链接逃逸）: {}",
                raw_path.display()
            );
        }
        // 累计解压字节（用条目 header 声明的 size——足够防 zip-bomb，且无需实际写盘即可判定）
        let size = entry
            .header()
            .size()
            .with_context(|| format!("读条目 size 失败: {}", raw_path.display()))?;
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > MAX_EXTRACTED_BYTES {
            bail!(
                "tar 解压累计 {total_bytes} 字节超上限 {MAX_EXTRACTED_BYTES}，拒绝（防 DoS）"
            );
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

/// 解析插件的传递依赖，返回**后序拓扑**（依赖在前，entry 自身最后）的安装顺序。
///
/// `dependencies: HashMap<id, version_req>`，按 semver `VersionReq` 在注册表中取
/// **最新满足版本**，递归解析。环检测（DFS 栈）；缺失/不满足 → Err。
/// 已在结果集中的 id 不重复（幂等）。
pub fn resolve_dependencies(
    entry: &RegistryEntry,
    registry: &PluginRegistryIndex,
) -> Result<Vec<RegistryEntry>> {
    let mut order: Vec<RegistryEntry> = Vec::new();
    let mut installed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stack: std::collections::HashSet<String> = std::collections::HashSet::new();
    resolve_recursive(entry, registry, &mut order, &mut installed, &mut stack)?;
    Ok(order)
}

fn resolve_recursive(
    entry: &RegistryEntry,
    registry: &PluginRegistryIndex,
    order: &mut Vec<RegistryEntry>,
    installed: &mut std::collections::HashSet<String>,
    stack: &mut std::collections::HashSet<String>,
) -> Result<()> {
    if installed.contains(&entry.id) {
        return Ok(()); // 已解析（钻石依赖去重）
    }
    if !stack.insert(entry.id.clone()) {
        bail!("插件依赖循环检测到: {}", entry.id);
    }
    for (dep_id, req_str) in &entry.dependencies {
        let req = VersionReq::parse(req_str)
            .with_context(|| format!("依赖 {} 的 version_req 非法: {}", dep_id, req_str))?;
        let dep = find_matching(registry, dep_id, &req)?;
        resolve_recursive(&dep, registry, order, installed, stack)?;
    }
    stack.remove(&entry.id);
    if installed.insert(entry.id.clone()) {
        order.push(entry.clone()); // 后序：依赖已 push，自身最后
    }
    Ok(())
}

/// 在注册表中找 `dep_id` 最新满足 `req` 的版本（find_all_versions 已降序）。
fn find_matching(
    registry: &PluginRegistryIndex,
    dep_id: &str,
    req: &VersionReq,
) -> Result<RegistryEntry> {
    registry
        .find_all_versions(dep_id)
        .into_iter()
        .find_map(|e| {
            Version::parse(&e.version)
                .ok()
                .filter(|v| req.matches(v))
                .map(|_| e.clone())
        })
        .ok_or_else(|| anyhow::anyhow!("未找到满足依赖 {} {} 的注册表条目", dep_id, req))
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
    fn test_multi_version_find_latest_and_compatible() {
        // 多版本：把 another-plugin 改成 example-plugin 的另一版本（2.1.3）
        let toml = valid_registry_toml()
            .replacen("id = \"another-plugin\"", "id = \"example-plugin\"", 1);
        let idx = PluginRegistryIndex::parse(&toml).unwrap(); // 多版本（同 id）允许

        // find_latest 取最高版本
        assert_eq!(
            idx.find_latest("example-plugin").map(|e| e.version.as_str()),
            Some("2.1.3")
        );
        // find_version 取指定版本（锁定/回滚）
        let v100 = Version::parse("1.0.0").unwrap();
        assert_eq!(
            idx.find_version("example-plugin", &v100).map(|e| e.version.as_str()),
            Some("1.0.0")
        );
        // latest_compatible 去重：两个 example-plugin 条目 → 一个最高兼容版
        let host = Version::parse("0.1.0").unwrap();
        let latest = idx.latest_compatible(&host, "1");
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].version, "2.1.3");
    }

    #[test]
    fn test_find_all_versions_desc() {
        let toml = valid_registry_toml()
            .replacen("id = \"another-plugin\"", "id = \"example-plugin\"", 1);
        let idx = PluginRegistryIndex::parse(&toml).unwrap();
        let versions = idx.find_all_versions("example-plugin");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, "2.1.3"); // 降序：最高在前
        assert_eq!(versions[1].version, "1.0.0");
        assert!(idx.find_all_versions("nope").is_empty());
    }

    /// 构造合法注册表条目 TOML（deps 为内联 dependencies 表内容，如 `y = "^1.0.0"`，或空）
    fn entry_toml(id: &str, version: &str, deps: &str) -> String {
        let deps_line = if deps.is_empty() {
            String::new()
        } else {
            format!("\ndependencies = {{ {} }}", deps)
        };
        format!(
            r#"[[plugins]]
id = "{id}"
name = "{id}"
version = "{version}"
description = ""
author = ""
license = "MIT"
abi_version = "1"
min_host_version = "0.1.0"
download_url = "https://example.com/{id}-{version}.tar.gz"
sha256 = "{sha}"
signature = "{sig}"
capabilities = []{deps_line}
"#,
            sha = "0".repeat(64),
            sig = "0".repeat(128),
        )
    }

    #[test]
    fn test_resolve_dependencies_post_order_and_version_req() {
        // x(1.0.0)→y(^1.0.0)；注册表 y@1.5.0 + y@2.0.0；^1.0.0 选 1.5.0（非 2.0.0）；
        // y(1.5.0)→z(^1.0.0)→z@1.0.0
        let toml = format!(
            "version = 1\n\n{}\n{}\n{}\n{}",
            entry_toml("x", "1.0.0", "y = \"^1.0.0\""),
            entry_toml("y", "1.5.0", "z = \"^1.0.0\""),
            entry_toml("y", "2.0.0", ""),
            entry_toml("z", "1.0.0", ""),
        );
        let idx = PluginRegistryIndex::parse(&toml).unwrap();
        let x = idx.find_latest("x").unwrap();
        let order = resolve_dependencies(x, &idx).unwrap();
        // 后序：z → y(1.5.0) → x
        assert_eq!(order.len(), 3);
        assert_eq!(order[0].id, "z");
        assert_eq!(order[1].id, "y");
        assert_eq!(order[1].version, "1.5.0"); // ^1.0.0 选 1.5.0 而非 2.0.0
        assert_eq!(order[2].id, "x");
    }

    #[test]
    fn test_resolve_dependencies_detects_cycle() {
        let toml = format!(
            "version = 1\n\n{}\n{}",
            entry_toml("a", "1.0.0", "b = \"^1.0.0\""),
            entry_toml("b", "1.0.0", "a = \"^1.0.0\""),
        );
        let idx = PluginRegistryIndex::parse(&toml).unwrap();
        let a = idx.find_latest("a").unwrap();
        assert!(resolve_dependencies(a, &idx).is_err());
    }

    #[test]
    fn test_resolve_dependencies_missing() {
        let toml = format!(
            "version = 1\n\n{}",
            entry_toml("x", "1.0.0", "w = \"^1.0.0\""), // 无 w
        );
        let idx = PluginRegistryIndex::parse(&toml).unwrap();
        let x = idx.find_latest("x").unwrap();
        assert!(resolve_dependencies(x, &idx).is_err());
    }

    #[test]
    fn test_resolve_dependencies_unsatisfied_version_constraint() {
        // app 依赖 lib ^1.0.0，但注册表仅 lib@2.0.0（^1.0.0 = >=1.0.0,<2.0.0，不匹配）
        // → find_matching 遍历所有版本均不满足 → Err，而非误装 2.0.0 或静默通过。
        // 安全关键：依赖版本范围不被满足时必须干净失败。
        let toml = format!(
            "version = 1\n\n{}\n{}",
            entry_toml("app", "1.0.0", "lib = \"^1.0.0\""),
            entry_toml("lib", "2.0.0", ""),
        );
        let idx = PluginRegistryIndex::parse(&toml).unwrap();
        let app = idx.find_latest("app").unwrap();
        assert!(
            resolve_dependencies(app, &idx).is_err(),
            "依赖版本约束不满足时应报错，而非误装 lib@2.0.0"
        );
    }

    #[test]
    fn test_resolve_dependencies_diamond_idempotent() {
        // 钻石依赖：a→b→c，a→c（c 经两条路径到达）。幂等：c 只入序一次。
        let toml = format!(
            "version = 1\n\n{}\n{}\n{}",
            entry_toml("a", "1.0.0", "b = \"^1.0.0\", c = \"^1.0.0\""),
            entry_toml("b", "1.0.0", "c = \"^1.0.0\""),
            entry_toml("c", "1.0.0", ""),
        );
        let idx = PluginRegistryIndex::parse(&toml).unwrap();
        let a = idx.find_latest("a").unwrap();
        let order = resolve_dependencies(a, &idx).unwrap();
        // 后序 + 去重：c（最先无依赖）→ b → a；c 仅一次
        assert_eq!(order.len(), 3);
        assert_eq!(order.iter().filter(|e| e.id == "c").count(), 1);
        assert_eq!(order[0].id, "c");
        assert_eq!(order[1].id, "b");
        assert_eq!(order[2].id, "a");
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
    fn test_find_latest_by_id() {
        let idx = PluginRegistryIndex::parse(&valid_registry_toml()).unwrap();
        assert_eq!(
            idx.find_latest("example-plugin").map(|e| e.version.as_str()),
            Some("1.0.0")
        );
        assert!(idx.find_latest("nope").is_none());
    }

    #[test]
    fn test_is_hex_of_len() {
        assert!(is_hex_of_len("ab12", 4));
        assert!(!is_hex_of_len("ab12", 5)); // 长度错
        assert!(!is_hex_of_len("xy", 2)); // 非 hex
    }

    #[test]
    fn test_check_updates() {
        let idx = PluginRegistryIndex::parse(&valid_registry_toml()).unwrap();
        // registry: example-plugin@1.0.0, another-plugin@2.1.3

        // 已装旧版 → 有更新
        let up = check_updates(&[("example-plugin".to_string(), "0.9.0".to_string())], &idx);
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].available_version, "1.0.0");
        assert_eq!(up[0].installed_version, "0.9.0");

        // 已装同版 → 无更新
        assert!(check_updates(&[("example-plugin".to_string(), "1.0.0".to_string())], &idx).is_empty());
        // 已装更新版（registry 旧）→ 无更新
        assert!(check_updates(&[("example-plugin".to_string(), "2.0.0".to_string())], &idx).is_empty());
        // 注册表无此 id → 无更新
        assert!(check_updates(&[("not-in-registry".to_string(), "1.0.0".to_string())], &idx).is_empty());
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

    /// 构造含一个 symlink 条目的 tar.gz（合法相对路径 `link_path` → 逃逸 target）。
    /// 用 raw `Builder::append` + 手配 Symlink header，绕过 append_data 的常规文件假设。
    fn build_tar_gz_symlink(link_path: &str, target: &str) -> Vec<u8> {
        let mut tar_buf = Vec::new();
        {
            let mut builder = Builder::new(&mut tar_buf);
            let mut header = Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_mode(0o777);
            header.set_size(0);
            header.set_path(link_path).unwrap();
            header.set_link_name(target).unwrap();
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
            builder.finish().unwrap();
        }
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&tar_buf).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn test_safe_extract_rejects_symlink() {
        // 合法相对路径的 symlink 指向 dest 外 → 必须拒绝落地（闭环 R1）。
        // tar 0.4.46 validate_inside_dst 仅防解压期逃逸，不阻止 symlink 落地；
        // 落地后会被 write_file 经字面校验利用。故 safe_extract 须显式拒 symlink 条目。
        let archive = build_tar_gz_symlink("lnk", "/etc/cron.d");
        let dest = TempDir::new().unwrap();
        let err = safe_extract_tar_gz(&archive, dest.path()).unwrap_err();
        assert!(
            err.to_string().contains("symlink"),
            "应明确拒绝 symlink 条目: {err}"
        );
        assert!(!dest.path().join("lnk").exists(), "symlink 不应落地");
    }

    #[test]
    fn test_safe_extract_rejects_too_many_entries() {
        // MAX_TAR_ENTRIES + 1 个空条目 → 第 4097 个触发条目数上限中止（防海量条目 DoS）。
        let names: Vec<String> = (0..(MAX_TAR_ENTRIES + 1)).map(|i| format!("f{i}")).collect();
        let entries: Vec<(&str, &[u8])> = names
            .iter()
            .map(|n| (n.as_str(), b"" as &[u8]))
            .collect();
        let archive = build_tar_gz(&entries);
        let dest = TempDir::new().unwrap();
        let err = safe_extract_tar_gz(&archive, dest.path()).unwrap_err();
        assert!(
            err.to_string().contains("条目数"),
            "应因条目数超限拒绝: {err}"
        );
    }

    // ── http_fetch SSRF 防护测试 ──────────────────────────────────────────────
    // http_fetch 在发请求前调用 extract_host + is_internal_ip 校验；
    // 内网 IP 字面量（loopback / 私网 / 云元数据 / IPv6 loopback）须在请求前就拒绝，
    // 不等待 TCP 连接失败（后者不可靠且不携带安全语义）。

    #[tokio::test]
    async fn test_http_fetch_rejects_http_scheme() {
        // http:// URL → 在 scheme 检查阶段就拒绝（防明文 MitM）
        let client = reqwest::Client::new();
        let err = http_fetch(&client, "http://example.com/plugin.tar.gz", 1024)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("https"),
            "http:// 应被 scheme 守卫拒绝: {err}"
        );
    }

    #[tokio::test]
    async fn test_http_fetch_rejects_internal_ipv4_literal() {
        // 127.0.0.1（loopback）→ 在连接前即被 SSRF 守卫拒绝
        let client = reqwest::Client::new();
        let err = http_fetch(&client, "https://127.0.0.1/payload.tar.gz", 1024)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("SSRF") || err.to_string().contains("内网"),
            "127.0.0.1 应被 SSRF 守卫拒绝: {err}"
        );
    }

    #[tokio::test]
    async fn test_http_fetch_rejects_private_network_literal() {
        // 192.168.x.x（私网）→ 拒绝
        let client = reqwest::Client::new();
        let err = http_fetch(&client, "https://192.168.1.100/evil.tar.gz", 1024)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("SSRF") || err.to_string().contains("内网"),
            "192.168.x.x 应被 SSRF 守卫拒绝: {err}"
        );
    }

    #[tokio::test]
    async fn test_http_fetch_rejects_cloud_metadata_literal() {
        // 169.254.169.254（AWS/GCP/Azure 云元数据服务）→ 链路本地地址，必须拒绝
        let client = reqwest::Client::new();
        let err = http_fetch(&client, "https://169.254.169.254/latest/meta-data/", 1024)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("SSRF") || err.to_string().contains("内网"),
            "169.254.169.254 应被 SSRF 守卫拒绝: {err}"
        );
    }

    #[tokio::test]
    async fn test_http_fetch_rejects_ipv6_loopback_literal() {
        // [::1]（IPv6 loopback）→ 拒绝
        let client = reqwest::Client::new();
        let err = http_fetch(&client, "https://[::1]/evil.tar.gz", 1024)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("SSRF") || err.to_string().contains("内网"),
            "[::1] 应被 SSRF 守卫拒绝: {err}"
        );
    }
}
