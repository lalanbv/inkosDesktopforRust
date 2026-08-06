//! M3c：运行时 Node 自适应 bootstrap。
//!
//! 首启（或缓存未命中）按用户平台/arch 下载便携 Node：
//! - **region-aware mirror**：默认 nodejs.org 官方；检测到 CN（locale/时区）优先 npmmirror。
//! - **安全校验**：下载的二进制始终用**官方** nodejs.org `SHASUMS256.txt`（HTTPS）校验
//!   ——即使二进制取自 mirror，官方 checksum 是唯一可信源，mirror 无法伪造。
//! - **本地缓存**：解压到 `app_data/runtime/node/{ver}-{os}-{arch}/`，后续启动直接命中。
//! - **降级**：bootstrap 失败（无网/校验不过/解压失败）→ 调用方回退系统 `node`（log 不阻塞）。
//!
//! `resolve` 为 **async**（用 async reqwest）——避免 blocking client 在 Tauri async_runtime
//! 任务内的 "Cannot start a runtime within a runtime" panic。调用方在 spawn_sidecar_task 内 await。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::engine::manifest::sha256_file;

/// bootstrap 下载的 Node 版本（LTS 22.x，可复现 + 可校验）。
/// 升级：改此常量 + M3e CI 验证。比"拉 latest"更安全（固定 SHASUMS 可校验）；mirror 给区域速度。
pub const NODE_BOOTSTRAP_VERSION: &str = "22.11.0";

/// 官方 nodejs.org dist 根（SHASUMS + 二进制权威源）。始终用于 SHASUMS 校验。
pub const OFFICIAL_DIST_URL: &str = "https://nodejs.org/dist";

/// 进度回调（阶段文本，例如 "下载 45%" / "校验 SHA256" / "解压"）。None = 静默。
pub type ProgressFn = Arc<dyn Fn(&str) + Send + Sync>;

// =====================================================================
// 平台/架构检测（cfg-based，纯逻辑）
// =====================================================================

/// 目标平台 + 架构 + 归档扩展（决定 tarball 名与解压方式）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformArch {
    pub os: &'static str,    // darwin / linux / win
    pub arch: &'static str,  // x64 / arm64
    pub archive_ext: &'static str, // tar.gz / zip
}

impl PlatformArch {
    pub fn detect() -> Self {
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "win"
        } else {
            "linux"
        };
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        let archive_ext = if cfg!(target_os = "windows") {
            "zip"
        } else {
            "tar.gz"
        };
        Self { os, arch, archive_ext }
    }

    /// 缓存目录键：`{ver}-{os}-{arch}`。
    pub fn cache_key(&self, ver: &str) -> String {
        format!("{ver}-{}-{}", self.os, self.arch)
    }

    /// tarball 文件名：`node-v{ver}-{os}-{arch}.{ext}`（对齐 nodejs.org 命名）。
    pub fn tarball_name(&self, ver: &str) -> String {
        format!("node-v{ver}-{}-{}.{}", self.os, self.arch, self.archive_ext)
    }
}

// =====================================================================
// Mirror 选择（region-aware，纯逻辑——真正的扩展点）
// =====================================================================

/// Node 二进制下载 mirror 候选（有序：首个最优）。SHASUMS 始终官方（见模块文档）。
pub trait MirrorSelector: Send + Sync {
    fn binary_mirrors(&self) -> Vec<&'static str>;
}

/// 基于 locale/时区判定区域，选 mirror 顺序。可经 `INKOS_NODE_MIRROR` env 覆盖。
pub struct LocaleMirrorSelector;

const CN_MIRROR: &str = "https://cdn.npmmirror.com/binaries/node";

impl MirrorSelector for LocaleMirrorSelector {
    fn binary_mirrors(&self) -> Vec<&'static str> {
        if let Ok(custom) = std::env::var("INKOS_NODE_MIRROR") {
            // 泄漏到 'static：用户显式覆盖，进程级唯一，bootstrap 低频，可接受。
            let leaked: &'static str = Box::leak(custom.into_boxed_str());
            return vec![leaked];
        }
        if Self::is_cn_region() {
            vec![CN_MIRROR, OFFICIAL_DIST_URL]
        } else {
            vec![OFFICIAL_DIST_URL, CN_MIRROR]
        }
    }
}

impl LocaleMirrorSelector {
    /// CN 区域：locale 含 zh/CN，或时区 Asia/Shanghai|Beijing|Chongqing|Urumqi|Harbin。
    pub fn is_cn_region() -> bool {
        let locale = std::env::var("LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .or_else(|_| std::env::var("LC_CTYPE"))
            .unwrap_or_default();
        if locale.to_lowercase().contains("zh") || locale.contains("CN") {
            return true;
        }
        let tz = std::env::var("TZ").unwrap_or_default();
        matches!(
            tz.as_str(),
            "Asia/Shanghai" | "Asia/Beijing" | "Asia/Chongqing" | "Asia/Urumqi" | "Asia/Harbin"
        )
    }
}

/// 拼装下载 URL：`{base}/v{ver}/{tarball}`。
pub fn binary_url(base: &str, ver: &str, tarball: &str) -> String {
    format!("{base}/v{ver}/{tarball}")
}

/// 官方 SHASUMS256.txt URL（始终官方，安全校验权威源）。
pub fn shasums_url(ver: &str) -> String {
    format!("{OFFICIAL_DIST_URL}/v{ver}/SHASUMS256.txt")
}

// =====================================================================
// SHASUMS256.txt 解析（纯逻辑）
// =====================================================================

/// 解析 SHASUMS256.txt：`<hex>  <filename>` → filename→hex。跳过空行/非法；取 basename。
pub fn parse_shasums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, |c: char| c.is_whitespace());
        let Some(hex) = parts.next().map(str::trim) else {
            continue;
        };
        let Some(name) = parts.next().map(str::trim) else {
            continue;
        };
        if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) && !name.is_empty() {
            let base = name.rsplit('/').next().unwrap_or(name);
            map.insert(base.to_string(), hex.to_lowercase());
        }
    }
    map
}

// =====================================================================
// Bootstrapping（async）
// =====================================================================

/// 下载 + 校验 + 解压 Node 到本地缓存，返回 node 二进制路径。
///
/// - 缓存命中 → 直接返回，无网络。
/// - 未命中 → mirror 列表逐个尝试下载 tarball，官方 SHASUMS256 校验，解压，返回 bin。
///
/// `selector: Box<dyn MirrorSelector>` 持有（扩展点：locale/企业镜像/固定源）。
pub struct BootstrappingResolver {
    pub cache_dir: PathBuf,
    pub selector: Box<dyn MirrorSelector>,
    pub progress: Option<ProgressFn>,
    pub client: reqwest::Client,
}

impl BootstrappingResolver {
    pub fn new(cache_dir: PathBuf, selector: Box<dyn MirrorSelector>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { cache_dir, selector, progress: None, client }
    }

    pub fn with_progress(mut self, progress: ProgressFn) -> Self {
        self.progress = Some(progress);
        self
    }

    fn emit(&self, msg: &str) {
        if let Some(p) = &self.progress {
            p(msg);
        }
    }

    /// 解压后 node 二进制相对路径（unix: node-v{ver}-{os}-{arch}/bin/node；
    /// windows: node-v{ver}-win-{arch}/node.exe）。
    fn node_bin_rel(pa: &PlatformArch, ver: &str) -> PathBuf {
        let top = format!("node-v{ver}-{}-{}", pa.os, pa.arch);
        if pa.archive_ext == "zip" {
            PathBuf::from(top).join("node.exe")
        } else {
            PathBuf::from(top).join("bin").join("node")
        }
    }

    pub async fn resolve(&self) -> Result<PathBuf> {
        let pa = PlatformArch::detect();
        let ver = NODE_BOOTSTRAP_VERSION;
        let key = pa.cache_key(ver);
        let key_dir = self.cache_dir.join(&key);
        let bin = key_dir.join(Self::node_bin_rel(&pa, ver));

        // 缓存命中。
        if bin.is_file() {
            self.emit("缓存命中");
            return Ok(bin);
        }

        let tarball = pa.tarball_name(ver);

        // 1) 官方 SHASUMS（安全权威源；不可达 → 不 bootstrap，调用方回退系统 node）。
        self.emit("下载 SHASUMS256（官方）");
        let shasums_text = self
            .client
            .get(shasums_url(ver))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("下载 SHASUMS256 失败 (ver={ver})"))?
            .text()
            .await
            .context("读取 SHASUMS256 体失败")?;
        let sums = parse_shasums(&shasums_text);
        let expected = sums.get(&tarball).with_context(|| {
            format!("SHASUMS256 未含 {tarball}（ver={ver} 不存在或非 LTS？）")
        })?;

        // 2) 从 mirror 列表逐个下载，校验通过即用。
        std::fs::create_dir_all(&key_dir)
            .with_context(|| format!("创建缓存目录失败: {}", key_dir.display()))?;
        let mirrors = self.selector.binary_mirrors();
        let mut last_err: Option<anyhow::Error> = None;
        for base in mirrors {
            let url = binary_url(base, ver, &tarball);
            self.emit(&format!("下载 node {ver}（{}）", short_host(base)));
            let tarball_path = key_dir.join(&tarball);
            match self.download(&url, &tarball_path).await {
                Ok(()) => {
                    self.emit("校验 SHA256");
                    let verified = match sha256_file(&tarball_path) {
                        Ok(actual) if actual == *expected => true,
                        Ok(actual) => {
                            last_err = Some(anyhow::anyhow!(
                                "SHA256 不匹配（mirror 可能被篡改）：期望 {expected}，实际 {actual}"
                            ));
                            false
                        }
                        Err(e) => {
                            last_err = Some(e);
                            false
                        }
                    };
                    if verified {
                        self.emit("解压");
                        match extract_archive(&tarball_path, &key_dir) {
                            Ok(()) => {
                                let _ = std::fs::remove_file(&tarball_path);
                                if bin.is_file() {
                                    self.emit("完成");
                                    return Ok(bin);
                                }
                                last_err = Some(anyhow::anyhow!(
                                    "解压后未找到 node 二进制: {}",
                                    bin.display()
                                ));
                            }
                            Err(e) => {
                                let _ = std::fs::remove_file(&tarball_path);
                                last_err = Some(e);
                            }
                        }
                    } else {
                        let _ = std::fs::remove_file(&tarball_path);
                    }
                }
                Err(e) => last_err = Some(e),
            }
        }
        // 全失败 → 清理半成品，调用方回退系统 node。
        let _ = std::fs::remove_dir_all(&key_dir);
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("无可用 mirror")))
    }

    /// 流式下载（chunk，避免 ~30MB tarball 一次性入内存）。
    async fn download(&self, url: &str, dest: &Path) -> Result<()> {
        let mut resp = self
            .client
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("下载失败: {url}"))?;
        let mut f = std::fs::File::create(dest)
            .with_context(|| format!("创建下载文件失败: {}", dest.display()))?;
        use std::io::Write;
        while let Some(chunk) = resp
            .chunk()
            .await
            .with_context(|| format!("读取下载流失败: {url}"))?
        {
            f.write_all(&chunk)
                .with_context(|| format!("写下载文件失败: {}", dest.display()))?;
        }
        Ok(())
    }
}

/// 解压 tarball 到 dest_dir（保留顶层 node-v{ver}-.../ 目录）。pub 供 updater engine 通道复用。
pub fn extract_archive(tarball: &Path, dest_dir: &Path) -> Result<()> {
    let is_gz = tarball
        .file_name()
        .map(|n| n.to_string_lossy().ends_with(".tar.gz"))
        .unwrap_or(false);
    if is_gz {
        let f = std::fs::File::open(tarball).context("打开 tarball 失败")?;
        let gz = flate2::read::GzDecoder::new(f);
        let mut archive = tar::Archive::new(gz);
        archive.unpack(dest_dir).context("解压 tar.gz 失败")?;
        Ok(())
    } else {
        extract_zip(tarball, dest_dir)
    }
}

#[cfg(windows)]
fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let f = std::fs::File::open(zip_path).context("打开 zip 失败")?;
    let mut archive = zip::ZipArchive::new(f).context("读 zip 失败")?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("读 zip entry {i} 失败"))?;
        let Some(enclosed) = entry.enclosed_name() else { continue }; // 防 zip slip
        let outpath = dest_dir.join(enclosed);
        if entry.is_dir() {
            std::fs::create_dir_all(&outpath).ok();
        } else {
            std::fs::create_dir_all(outpath.parent().unwrap_or(dest_dir)).ok();
            let mut outfile = std::fs::File::create(&outpath)
                .with_context(|| format!("创建 {} 失败", outpath.display()))?;
            std::io::copy(&mut entry, &mut outfile).context("写 zip entry 失败")?;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn extract_zip(_zip_path: &Path, _dest_dir: &Path) -> Result<()> {
    anyhow::bail!("zip 解压仅 Windows 编译（当前目标非 windows）")
}

fn short_host(base: &str) -> &str {
    base.strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
        .unwrap_or(base)
        .split('/')
        .next()
        .unwrap_or(base)
}


#[cfg(test)]
#[path = "tests.rs"]
mod tests;
