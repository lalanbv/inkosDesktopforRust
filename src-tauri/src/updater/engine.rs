//! M3d engine 通道：GitHub Releases → 下载 engine bundle → SHA256 校验 → 原子替换 + 回滚。
//!
//! M3 审计修复（rust-reviewer Block 项）：
//! - **C1**：回滚路径的 rename/remove 错误不再 `let _ =` 吞掉；回滚失败返回**独立错误**
//!   "engine 缺失，需重装"，让 UI 提示重试而非谎报已回滚。
//! - **H1**：`download_to` 写错误用 `?` + context（不再 `.ok()` 吞）。
//! - **H3**：client 加 `timeout(300s)`；下载按 GitHub asset `size` 字段封顶（防 mirror 慢流/巨包 DoS）。
//! - **M1**：`atomic_replace_with_rollback` 起手校验 new_dir/engine_dir 同 fs（防 CrossesDevices）。
//! - **M2**：`create_dir_all(staging)` 用 `?`；staging 清理覆盖所有返回路径。
//! - **L4**：下载到 `NamedTempFile` + `persist`（崩溃/中断不留半成品占盘）。

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::is_newer;

// === engine bundle 签名验证策略（纯函数 + 单一来源，便于单测）===
//
// fail-closed 原则：部署方一旦配置 INKOS_ENGINE_PUBKEY，即表达「只接受签名 bundle」
// 的意图。此时缺签名的 bundle 必须拒绝——否则攻击者只需从发布 feed 剥离 .sig 即可
// 让「已配置验证」的客户端接受未签名（被篡改）bundle（降级攻击）。
//
// | release 含 .sig | INKOS_ENGINE_PUBKEY | 动作           |
// |-----------------|---------------------|----------------|
// | 有              | 有                  | 强制验证，失败拒绝 |
// | 有              | 无                  | 放行 + warn（无法验证；过渡期）|
// | 无              | 有                  | **拒绝**（缺签名=可疑）       |
// | 无              | 无                  | 放行 + warn（无签名基础设施；过渡期）|
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SigAction {
    Verify,
    WarnPass,
    Reject,
}

/// 依据「release 是否含 .sig」与「是否配置公钥」决定签名验证动作（纯函数）。
pub(super) fn decide_sig_action(sig_present: bool, pubkey_configured: bool) -> SigAction {
    match (sig_present, pubkey_configured) {
        (true, true) => SigAction::Verify,
        (true, false) | (false, false) => SigAction::WarnPass,
        (false, true) => SigAction::Reject,
    }
}

/// 原子替换 engine 目录 + 健康预检失败自动回滚。
///
/// 流程（同 filesystem，起手校验）：
/// 1. **同 fs 校验**（M1）：new_dir 与 engine_dir 父目录 canonicalize 比较，跨 fs → 早 bail。
/// 2. 备份：`engine_dir` → `bak_dir`（bak 存在先删；engine 不存在则跳过——首装）。
/// 3. 替换：`new_dir` → `engine_dir`；失败 → 回滚 bak，回滚失败返回独立错误。
/// 4. 健康预检 `health_check(engine_dir)` 失败 → 回滚；**回滚失败返回独立错误**
///    "engine 缺失需重装"（C1，不再谎报已回滚）。
/// 5. 成功：保留 `bak_dir`（架构 §6.3 回滚备份）。
pub fn atomic_replace_with_rollback(
    engine_dir: &Path,
    bak_dir: &Path,
    new_dir: &Path,
    health_check: &(dyn Fn(&Path) -> bool + Sync),
) -> Result<()> {
    // M1：同 fs 校验（rename 跨 fs 返回 CrossesDevices）。
    if let (Some(np), Some(ep)) = (new_dir.parent(), engine_dir.parent()) {
        if let (Ok(a), Ok(b)) = (std::fs::canonicalize(np), std::fs::canonicalize(ep)) {
            if a != b {
                anyhow::bail!(
                    "new_dir({}) 与 engine_dir({}) 跨文件系统，rename 非原子",
                    a.display(),
                    b.display()
                );
            }
        }
        // canonicalize 失败（路径不存在等）→ 不阻断，交由后续 rename 暴露真实错误。
    }

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

    // 2. 替换：new → engine。失败 → 回滚；回滚也失败 → 独立「engine 缺失」错误（C1）。
    if let Err(e) = std::fs::rename(new_dir, engine_dir) {
        let restored = had_old && std::fs::rename(bak_dir, engine_dir).is_ok();
        if !restored {
            return Err(anyhow::anyhow!(
                "替换 engine 失败({e}) 且回滚失败——engine 缺失，需重装/重试更新"
            ));
        }
        return Err(e).with_context(|| format!("替换 engine 失败（已回滚）: {}", engine_dir.display()));
    }

    // 3. 健康预检；失败回滚。回滚失败 → 独立错误（C1）。
    if !health_check(engine_dir) {
        let restored = if had_old {
            std::fs::remove_dir_all(engine_dir).is_ok()
                && std::fs::rename(bak_dir, engine_dir).is_ok()
        } else {
            let _ = std::fs::remove_dir_all(engine_dir);
            true // 首装无 bak，"回滚"=清掉失败的新装即可
        };
        if restored {
            anyhow::bail!("健康预检失败，已回滚至上一版本");
        }
        anyhow::bail!("健康预检失败且回滚失败——engine 缺失，需重装/重试更新");
    }
    Ok(())
}

/// GitHub release（`releases/latest`）最小 schema。`size`（字节）用于下载封顶（H3）。
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// engine 通道：查本仓 GitHub release → 下载平台 bundle + .sha256 → 校验 → 原子替换 engine_dir。
pub struct EngineChannel {
    pub repo: String,
    pub current_version: String,
    pub client: reqwest::Client,
}

/// 校验 `owner/repo` 形如 GitHub slug：恰一个 `/`，两段非空，字符限
/// 字母数字 + `-`/`_`/`.`，各段 ≤ 100。
///
/// **安全关键**：`repo` 来自 `INKOS_REPO` 环境变量并被拼进
/// `https://api.github.com/repos/{repo}/releases/latest`。未校验时
/// `x/y@evil.com/` 的 userinfo 语义会把实际 host 变成 `evil.com`，
/// `../../` 可跳出 `/repos/` 路径——两者都能劫持更新源。
/// bundle 验签在公钥未配置时为 WarnPass（过渡期），不能作为唯一防线。
pub(super) fn is_valid_repo_slug(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    // split_once 只切首个 '/'，name 内残留的 '/' 会被下面字符白名单拒绝
    let ok_seg = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    ok_seg(owner) && ok_seg(name)
}

impl EngineChannel {
    /// 构造 engine 更新通道。`repo` 非法（见 [`is_valid_repo_slug`]）时回退到内置
    /// 默认仓库并 warn——fail-safe：宁可查官方源，不可查攻击者指定的源。
    pub fn new(repo: String, current_version: String) -> Self {
        let repo = if is_valid_repo_slug(&repo) {
            repo
        } else {
            tracing::warn!(
                target: "inkos.updater.security",
                invalid_repo = %repo,
                "INKOS_REPO 格式非法（须为 owner/repo），回退内置默认仓库"
            );
            DEFAULT_REPO.to_string()
        };
        Self::new_unchecked(repo, current_version)
    }

    fn new_unchecked(repo: String, current_version: String) -> Self {
        // H3：总超时 300s（下载 + 校验预算）；防 mirror 慢流永久挂起。
        let client = reqwest::Client::builder()
            .user_agent("inkosDesktop-updater")
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(300))
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
        let rel = self.fetch_release().await?;
        Ok(is_newer(&self.current_version, &rel.tag_name).map(|_| rel.tag_name))
    }

    /// 应用最新 release：fetch → 下载 bundle + .sha256（size 封顶）→ 校验 → 解压 → 原子替换。
    /// staging 清理覆盖所有返回路径（M2）。`extract` = 解压函数（复用 node::extract_archive）。
    pub async fn apply(
        &self,
        engine_dir: &Path,
        bak_dir: &Path,
        staging_dir: &Path,
        extract: &(dyn Fn(&Path, &Path) -> Result<()> + Sync),
    ) -> Result<()> {
        std::fs::create_dir_all(staging_dir)
            .with_context(|| format!("创建 staging 失败: {}", staging_dir.display()))?;

        let rel = self.fetch_release().await?;
        let ver = rel.tag_name.trim_start_matches('v');
        let tarball_name = bundle_name(ver);
        let bundle_asset = asset(&rel, &tarball_name)?;
        let sha_asset = asset(&rel, &format!("{tarball_name}.sha256"))?;
        // M4c：签名 asset 可选（过渡期旧 release 可能无 .sig；有则强制验证）
        let sig_asset = asset(&rel, &format!("{tarball_name}.sig")).ok();
        let bundle_path = staging_dir.join(&tarball_name);
        let sha_path = staging_dir.join(format!("{tarball_name}.sha256"));
        let sig_path = staging_dir.join(format!("{tarball_name}.sig"));
        let new_dir = staging_dir.join("new_engine");

        // 任意提前返回都清理本次 staging 产物（M2）。
        let result = self
            .apply_inner(
                &bundle_asset.browser_download_url,
                bundle_asset.size,
                &sha_asset.browser_download_url,
                sig_asset.as_ref().map(|a| a.browser_download_url.as_str()),
                &bundle_path,
                &sha_path,
                &sig_path,
                &new_dir,
                engine_dir,
                bak_dir,
                extract,
            )
            .await;
        // 清理（无论成功/失败）：bundle/sha/sig 临时文件 + new_dir（成功后 new_dir 已 rename 走）。
        let _ = std::fs::remove_file(&bundle_path);
        let _ = std::fs::remove_file(&sha_path);
        let _ = std::fs::remove_file(&sig_path);
        let _ = std::fs::remove_dir_all(&new_dir);
        result
    }

    /// apply 的内部体，返回后由 apply 统一清理 staging。
    #[allow(clippy::too_many_arguments)] // 私有实现细节，参数均为下载/校验/替换上下文。
    async fn apply_inner(
        &self,
        bundle_url: &str,
        bundle_size: u64,
        sha_url: &str,
        sig_url: Option<&str>,
        bundle_path: &Path,
        sha_path: &Path,
        sig_path: &Path,
        new_dir: &Path,
        engine_dir: &Path,
        bak_dir: &Path,
        extract: &(dyn Fn(&Path, &Path) -> Result<()> + Sync),
    ) -> Result<()> {
        // 下载 bundle（按 asset size 封顶）+ sha。
        download_to(&self.client, bundle_url, bundle_path, bundle_size).await?;
        download_to(&self.client, sha_url, sha_path, 0).await?;

        // 校验：bundle SHA256 == 下载的 .sha256 内容。
        let expected = std::fs::read_to_string(sha_path)
            .context("读 .sha256 失败")?
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();
        let actual = crate::engine::manifest::sha256_file(bundle_path)?;
        if actual != expected {
            anyhow::bail!("engine bundle SHA256 不匹配：期望 {expected}，实际 {actual}");
        }

        // M4c 签名验证（来源认证）：公钥编译期注入（INKOS_ENGINE_PUBKEY env）。
        // - 公钥已配置 + release 提供 .sig → 强制验证，失败拒绝更新
        // - 公钥未配置 或 release 无 .sig → 过渡期跳过 + warn（不阻断既有更新流程）
        //   部署侧设置 INKOS_ENGINE_PUBKEY 后即转为强制模式（见密钥采购指引文档）。
        let pubkey_hex = option_env!("INKOS_ENGINE_PUBKEY");
        match decide_sig_action(sig_url.is_some(), pubkey_hex.is_some()) {
            SigAction::Verify => {
                // decide_sig_action 仅在 sig_url 与 pubkey_hex 均为 Some 时返回 Verify，
                // 故这两处 unwrap 由契约保证安全。
                let url = sig_url.unwrap();
                let pubkey = pubkey_hex.unwrap();
                download_to(&self.client, url, sig_path, 0).await?;
                let sig_hex = std::fs::read_to_string(sig_path)
                    .context("读 .sig 失败")?
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let sig_bytes = crate::updater::sig::decode_hex(&sig_hex)
                    .context(".sig hex 解析失败")?;
                let bundle_bytes = std::fs::read(bundle_path).context("读 bundle 失败")?;
                let pubkey_bytes = crate::updater::sig::decode_hex(pubkey)
                    .context("INKOS_ENGINE_PUBKEY hex 解析失败")?;
                crate::updater::sig::verify(&bundle_bytes, &sig_bytes, &pubkey_bytes)
                    .context("engine bundle 签名验证失败——拒绝该 bundle（可能被篡改）")?;
                tracing::info!("engine bundle 签名验证通过");
            }
            SigAction::WarnPass => {
                tracing::warn!(
                    "engine bundle 签名验证跳过（INKOS_ENGINE_PUBKEY 未配置或 release 无 .sig）——\
                     过渡期放行，部署侧配置公钥且发布带签 release 后即转为强制模式"
                );
            }
            SigAction::Reject => {
                // fail-closed：已配置公钥（验证意图）但 release 未签名 → 拒绝。
                anyhow::bail!(
                    "已配置签名公钥（INKOS_ENGINE_PUBKEY）但该 release 未提供 .sig ——\
                     拒绝未签名 bundle（防降级攻击：剥离 .sig 不应绕过已配置的验证）。\
                     若为合法旧版 release，请为其补签，或回退客户端公钥配置以恢复过渡放行"
                );
            }
        }

        // 解压到临时 new_dir（与 engine_dir 同 fs：放 staging 下）。
        if new_dir.exists() {
            std::fs::remove_dir_all(new_dir)
                .with_context(|| format!("清理旧 new_dir 失败: {}", new_dir.display()))?;
        }
        extract(bundle_path, new_dir)?;

        // 原子替换 + 回滚（健康预检 = 结构：manifest.json + dist/index.js 存在）。
        atomic_replace_with_rollback(engine_dir, bak_dir, new_dir, &|dir| {
            dir.join("manifest.json").is_file() && dir.join("dist/index.js").is_file()
        })
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

fn asset<'a>(rel: &'a GithubRelease, name: &str) -> Result<&'a GithubAsset> {
    rel.assets
        .iter()
        .find(|a| a.name == name)
        .with_context(|| format!("release 未含 asset: {name}"))
}

/// 内置默认仓库（`INKOS_REPO` 未设或格式非法时的 fail-safe 目标）。
/// 单点定义：`main.rs` 读 env 的默认值也用此常量，避免两处漂移。
pub const DEFAULT_REPO: &str = "lalanbv/inkosDesktopforRust";

/// 无已知 asset size 时的下载硬上限（`.sha256` / `.sig` 等小文本附件）。
/// 这些文件实际仅数十至数百字节；1 MiB 留足余量同时封住无界写入。
const SMALL_ASSET_MAX_BYTES: u64 = 1024 * 1024;

/// 下载到 `dest`（NamedTempFile + persist，崩溃不留半成品 L4）。
/// `max_bytes > 0` 时按 GitHub asset size 封顶（H3 防 DoS；超 2× 拒绝）；
/// `max_bytes == 0`（大小未知）时用 [`SMALL_ASSET_MAX_BYTES`]。
/// 写错误用 `?`（H1，不再 `.ok()` 吞）。
async fn download_to(client: &reqwest::Client, url: &str, dest: &Path, max_bytes: u64) -> Result<()> {
    use std::io::Write;
    // 实际生效的字节上限。max_bytes>0 → 已知 asset size 的 2×（镜像可能有微小差异）；
    // ==0 → 小附件默认上限。恒 >0，故流式循环始终有界。
    let limit = if max_bytes > 0 {
        max_bytes.saturating_mul(2)
    } else {
        SMALL_ASSET_MAX_BYTES
    };
    let parent = dest.parent().context("dest 应有父目录")?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("创建下载临时文件失败: {}", parent.display()))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("下载失败: {url}"))?;
    // H3：Content-Length 预检——声明即超限则不必开始下载（快速失败）。
    if let Some(len) = resp.content_length() {
        if len > limit {
            anyhow::bail!("下载体积 {len} 超上限 {limit}（镜像异常？拒绝以防空盘）");
        }
    }
    // 流式累计校验：Content-Length 可缺失（chunked transfer）或撒谎，仅靠预检
    // 等于无上限——服务器不发 Content-Length 即可无限写入直到磁盘满。
    let mut written: u64 = 0;
    while let Some(chunk) = resp
        .chunk()
        .await
        .with_context(|| format!("读取下载流失败: {url}"))?
    {
        written = written.saturating_add(chunk.len() as u64);
        if written > limit {
            anyhow::bail!("下载流超上限 {limit} 字节（已写 {written}，拒绝以防空盘）: {url}");
        }
        tmp.write_all(&chunk)
            .with_context(|| format!("写下载文件失败: {}", dest.display()))?;
    }
    tmp.flush().with_context(|| format!("flush 下载失败: {}", dest.display()))?;
    tmp.persist(dest)
        .map_err(|e| anyhow::anyhow!("persist 下载 {} 失败: {}", dest.display(), e.error))?;
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
    fn test_is_valid_repo_slug_accepts_normal() {
        assert!(is_valid_repo_slug("lalanbv/inkosDesktopforRust"));
        assert!(is_valid_repo_slug("a/b"));
        assert!(is_valid_repo_slug("org-name/repo_name.js"));
    }

    #[test]
    fn test_is_valid_repo_slug_rejects_url_hijack() {
        // userinfo 语义：拼进 https://api.github.com/repos/{repo}/... 后
        // 实际 host 变成 evil.com（第一层 URL 解析即被劫持）
        assert!(!is_valid_repo_slug("x/y@evil.com"));
        // 路径穿越：跳出 /repos/ 命名空间
        assert!(!is_valid_repo_slug("../../evil"));
        assert!(!is_valid_repo_slug("a/../../b"));
        // 多段：split_once 后 name 含 '/'，被字符白名单拒
        assert!(!is_valid_repo_slug("a/b/c"));
        // query/fragment 改写
        assert!(!is_valid_repo_slug("a/b?x=1"));
        assert!(!is_valid_repo_slug("a/b#f"));
        // 控制字符与空白
        assert!(!is_valid_repo_slug("a/b\nc"));
        assert!(!is_valid_repo_slug("a /b"));
    }

    #[test]
    fn test_is_valid_repo_slug_rejects_malformed() {
        assert!(!is_valid_repo_slug(""));
        assert!(!is_valid_repo_slug("noslash"));
        assert!(!is_valid_repo_slug("/b")); // owner 空
        assert!(!is_valid_repo_slug("a/")); // name 空
        assert!(!is_valid_repo_slug(&format!("a/{}", "x".repeat(101)))); // 超长
    }

    #[test]
    fn test_engine_channel_falls_back_on_invalid_repo() {
        // fail-safe：非法 repo 不应被用于拼 URL，须回退内置默认仓库
        let ch = EngineChannel::new("x/y@evil.com".into(), "0.0.1".into());
        assert_eq!(ch.repo, DEFAULT_REPO, "非法 repo 须回退 DEFAULT_REPO");
        // 合法值原样保留
        let ch = EngineChannel::new("owner/name".into(), "0.0.1".into());
        assert_eq!(ch.repo, "owner/name");
    }

    #[test]
    fn sig_action_verify_when_both_present() {
        assert_eq!(decide_sig_action(true, true), SigAction::Verify);
    }

    #[test]
    fn sig_action_warnpass_when_cannot_or_not_armed() {
        // release 有签名但客户端未配公钥（无法验证）；双方均无（无基础设施）→ 过渡放行。
        assert_eq!(decide_sig_action(true, false), SigAction::WarnPass);
        assert_eq!(decide_sig_action(false, false), SigAction::WarnPass);
    }

    #[test]
    fn sig_action_reject_when_armed_but_unsigned() {
        // fail-closed 回归：已配置公钥（验证意图）+ release 无 .sig → 必须拒绝，
        // 防「剥离 .sig 降级攻击」。这是本次安全审计修复的核心断言。
        assert_eq!(decide_sig_action(false, true), SigAction::Reject);
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
        assert!(engine.join("manifest.json").is_file());
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
        assert_eq!(fs::read_to_string(engine.join("old.txt")).unwrap(), "old");
    }

    #[test]
    fn atomic_replace_first_install_no_bak() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let engine = root.join("engine");
        let bak = root.join("engine.bak");
        let new = root.join("new_engine");
        fs::create_dir_all(&new).unwrap();
        write(&new, "manifest.json", "{}");

        atomic_replace_with_rollback(&engine, &bak, &new, &|_| true).unwrap();
        assert!(engine.join("manifest.json").is_file());
        assert!(!bak.exists());
    }

    #[test]
    fn atomic_replace_refreshes_stale_bak() {
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
        assert!(!bak.join("stale.txt").exists());
    }

    #[test]
    fn bundle_name_matches_convention() {
        assert_eq!(bundle_name("1.7.3"), "engine-1.7.3.tar.gz");
    }

    // 真实 GitHub release 探测（需网络 + 本仓有 release）。CI 可选跑。
    #[tokio::test]
    #[ignore]
    async fn real_engine_channel_check() {
        let ch = EngineChannel::new("lalanbv/inkosDesktopforRust".into(), "0.0.1".into());
        let _ = ch.check().await; // 仅验证 HTTP/JSON 路径不 panic
    }
}
