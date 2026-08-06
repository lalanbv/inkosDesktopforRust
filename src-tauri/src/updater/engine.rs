//! M3d engine 通道：GitHub Releases → 下载 engine bundle → SHA256 校验 → 原子替换 + 回滚。
//!
//! 纯逻辑核心 [`atomic_replace_with_rollback`] 可 TDD（temp dir 模拟）；GitHub 网络部分
//! [`EngineChannel`] 为 async，真实发布 release 的 e2e 由 M3e CI 验证（本仓发布 engine bundle）。

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::is_newer;

/// 原子替换 engine 目录 + 健康预检失败自动回滚。
///
/// 流程（同 filesystem，调用方保证 new_dir/bak_dir/engine_dir 同 fs）：
/// 1. 备份：`engine_dir` → `bak_dir`（bak 存在先删；engine 不存在则跳过——首装）。
/// 2. 替换：`new_dir` → `engine_dir`（engine 已 rename 走，目标不存在，Windows 安全）。
/// 3. 健康预检 `health_check(engine_dir)` 失败 → 回滚：删 engine，`bak_dir` → `engine_dir`。
/// 4. 成功：保留 `bak_dir`（架构 §6.3 回滚备份；下次成功可清）。
///
/// `new_dir` 调用方负责已解压 + SHA256 已校验。本函数仅做原子交换 + 回滚。
pub fn atomic_replace_with_rollback(
    engine_dir: &Path,
    bak_dir: &Path,
    new_dir: &Path,
    health_check: &(dyn Fn(&Path) -> bool + Sync),
) -> Result<()> {
    // 1. 备份当前 engine（若存在）。
    if bak_dir.exists() {
        std::fs::remove_dir_all(bak_dir)
            .with_context(|| format!("清理旧 bak 失败: {}", bak_dir.display()))?;
    }
    let had_old = engine_dir.exists();
    if had_old {
        std::fs::rename(engine_dir, bak_dir)
            .with_context(|| format!("备份 engine → bak 失败: {}", bak_dir.display()))?;
    }

    // 2. 替换：new → engine。
    if let Err(e) = std::fs::rename(new_dir, engine_dir) {
        // 替换失败 → 回滚 bak。
        if had_old {
            let _ = std::fs::rename(bak_dir, engine_dir);
        }
        return Err(e).with_context(|| format!("替换 engine 失败: {}", engine_dir.display()));
    }

    // 3. 健康预检；失败回滚。
    if !health_check(engine_dir) {
        let _ = std::fs::remove_dir_all(engine_dir);
        if had_old {
            let _ = std::fs::rename(bak_dir, engine_dir);
            anyhow::bail!("健康预检失败，已回滚至上一版本");
        }
        anyhow::bail!("健康预检失败且无 bak 可回滚（首装）");
    }
    Ok(())
}

/// GitHub release（`releases/latest`）最小 schema。
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// engine 通道：查本仓 GitHub release → 下载平台 bundle + .sha256 → 校验 → 原子替换。
pub struct EngineChannel {
    /// "owner/repo"（本仓，如 "lalanbv/inkosDesktopforRust"）。
    pub repo: String,
    /// 当前 engine 版本（manifest.engine_version）。
    pub current_version: String,
    pub client: reqwest::Client,
}

impl EngineChannel {
    pub fn new(repo: String, current_version: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("inkosDesktop-updater")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            repo,
            current_version,
            client,
        }
    }

    /// 查 latest release tag；若新于 current 返回 tag（如 "v1.7.3"），否则 None。
    pub async fn check(&self) -> Result<Option<String>> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", self.repo);
        let rel: GithubRelease = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("GitHub releases/latest 请求失败: {url}"))?
            .json()
            .await
            .context("解析 GitHub release JSON 失败")?;
        Ok(is_newer(&self.current_version, &rel.tag_name).map(|_| rel.tag_name))
    }

    /// 应用最新 release：fetch release → 下载平台无关 bundle + .sha256 → 校验 → 解压 → 原子替换 engine_dir。
    /// `extract` = 解压函数（tar.gz/zip，复用 node::extract_archive）。
    pub async fn apply(
        &self,
        engine_dir: &Path,
        bak_dir: &Path,
        staging_dir: &Path,
        extract: &(dyn Fn(&Path, &Path) -> Result<()> + Sync),
    ) -> Result<()> {
        let rel = self.fetch_release().await?;
        let ver = rel.tag_name.trim_start_matches('v');
        let tarball_name = bundle_name(ver);
        let bundle_url = asset_url(&rel, &tarball_name)?.to_string();
        let sha_url = asset_url(&rel, &format!("{tarball_name}.sha256"))?.to_string();

        std::fs::create_dir_all(staging_dir).ok();
        let bundle_path = staging_dir.join(&tarball_name);
        let sha_path = staging_dir.join(format!("{tarball_name}.sha256"));

        // 下载 bundle + sha。
        download_to(&self.client, &bundle_url, &bundle_path).await?;
        download_to(&self.client, &sha_url, &sha_path).await?;

        // 校验：bundle SHA256 == 下载的 .sha256 内容。
        let expected = std::fs::read_to_string(&sha_path)
            .context("读 .sha256 失败")?
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        let actual = crate::engine::manifest::sha256_file(&bundle_path)?;
        if actual != expected {
            anyhow::bail!("engine bundle SHA256 不匹配：期望 {expected}，实际 {actual}");
        }

        // 解压到临时 new_dir（与 engine_dir 同 fs：放 staging 下）。
        let new_dir = staging_dir.join("new_engine");
        if new_dir.exists() {
            std::fs::remove_dir_all(&new_dir).ok();
        }
        extract(&bundle_path, &new_dir)?;

        // 原子替换 + 回滚（健康预检 = 结构：manifest.json + dist/index.js 存在）。
        atomic_replace_with_rollback(engine_dir, bak_dir, &new_dir, &|dir| {
            dir.join("manifest.json").is_file() && dir.join("dist/index.js").is_file()
        })?;
        // 清理 staging。
        let _ = std::fs::remove_file(&bundle_path);
        let _ = std::fs::remove_file(&sha_path);
        let _ = std::fs::remove_dir_all(&new_dir);
        Ok(())
    }

    async fn fetch_release(&self) -> Result<GithubRelease> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", self.repo);
        let rel: GithubRelease = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("GitHub releases/latest 请求失败: {url}"))?
            .json()
            .await
            .context("解析 GitHub release JSON 失败")?;
        Ok(rel)
    }
}

fn asset_url<'a>(rel: &'a GithubRelease, name: &str) -> Result<&'a str> {
    rel.assets
        .iter()
        .find(|a| a.name == name)
        .map(|a| a.browser_download_url.as_str())
        .with_context(|| format!("release 未含 asset: {name}"))
}

async fn download_to(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    let mut resp = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("下载失败: {url}"))?;
    let mut f = std::fs::File::create(dest)
        .with_context(|| format!("创建文件失败: {}", dest.display()))?;
    use std::io::Write;
    while let Some(chunk) = resp.chunk().await.with_context(|| format!("读取流失败: {url}"))? {
        f.write_all(&chunk).ok();
    }
    Ok(())
}

/// engine bundle 文件名：`engine-{ver}.tar.gz`（inkos dist 纯 JS，平台无关；单资产）。
/// M3e CI 按此命名发布 asset + `.sha256`。
pub fn bundle_name(ver: &str) -> String {
    format!("engine-{ver}.tar.gz")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn atomic_replace_succeeds_and_backs_up_old() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let engine = root.join("engine");
        let bak = root.join("engine.bak");
        let new = root.join("new_engine");
        fs::create_dir_all(&engine).unwrap();
        fs::create_dir_all(&new).unwrap();
        write(&engine, "old.txt", "old");
        write(&new, "manifest.json", "{}");
        fs::create_dir_all(new.join("dist")).unwrap();
        write(&new, "dist/index.js", "new");

        atomic_replace_with_rollback(&engine, &bak, &new, &|_| true).unwrap();
        // engine 现为 new 内容。
        assert!(engine.join("manifest.json").is_file());
        assert!(engine.join("dist/index.js").is_file());
        // bak 保留 old。
        assert_eq!(fs::read_to_string(bak.join("old.txt")).unwrap(), "old");
    }

    #[test]
    fn atomic_replace_rolls_back_on_health_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let engine = root.join("engine");
        let bak = root.join("engine.bak");
        let new = root.join("new_engine");
        fs::create_dir_all(&engine).unwrap();
        fs::create_dir_all(&new).unwrap();
        write(&engine, "old.txt", "old");
        write(&new, "manifest.json", "{}");

        let err = atomic_replace_with_rollback(&engine, &bak, &new, &|_| false).unwrap_err();
        assert!(format!("{err}").contains("回滚"));
        // engine 回滚为 old。
        assert_eq!(fs::read_to_string(engine.join("old.txt")).unwrap(), "old");
        assert!(!new.exists(), "new_dir 应已被 rename 走");
    }

    #[test]
    fn atomic_replace_first_install_no_bak() {
        // engine 不存在（首装）→ 无 bak；health 失败也无 bak 可回滚 → Err。
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let engine = root.join("engine");
        let bak = root.join("engine.bak");
        let new = root.join("new_engine");
        fs::create_dir_all(&new).unwrap();
        write(&new, "manifest.json", "{}");

        atomic_replace_with_rollback(&engine, &bak, &new, &|_| true).unwrap();
        assert!(engine.join("manifest.json").is_file());
        assert!(!bak.exists(), "首装无旧版，不应有 bak");
    }

    #[test]
    fn atomic_replace_refreshes_stale_bak() {
        // 旧 bak 存在 → 应先清理再备份当前 engine。
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let engine = root.join("engine");
        let bak = root.join("engine.bak");
        let new = root.join("new_engine");
        for d in [&engine, &bak, &new] {
            fs::create_dir_all(d).unwrap();
        }
        write(&engine, "cur.txt", "cur");
        write(&bak, "stale.txt", "stale");
        write(&new, "manifest.json", "{}");

        atomic_replace_with_rollback(&engine, &bak, &new, &|_| true).unwrap();
        assert_eq!(fs::read_to_string(bak.join("cur.txt")).unwrap(), "cur");
        assert!(!bak.join("stale.txt").exists(), "旧 bak 应被清理");
    }

    #[test]
    fn bundle_name_matches_convention() {
        assert_eq!(bundle_name("1.7.3"), "engine-1.7.3.tar.gz");
    }

    // 真实 GitHub release 探测（需网络 + 本仓有 release）。CI 可选跑。
    #[tokio::test]
    #[ignore]
    async fn real_engine_channel_check() {
        // 用本仓探测（若无 release → check 返回 Err，正常）。
        let ch = EngineChannel::new("lalanbv/inkosDesktopforRust".into(), "0.0.1".into());
        let _ = ch.check().await; // 仅验证 HTTP/JSON 路径不 panic
    }
}
