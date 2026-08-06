//! EngineManifest：engine 自包含运行时的版本与完整性元数据。
//!
//! 由 `desktop-package-engine.sh` 在打包时写入 `engine/manifest.json`；
//! M3d updater 下载新 engine bundle 后用 `engine_sha256` 校验、用 `engine_version`
//! 比对是否需更新。schema 见 M3 总体设计 §3.4。

use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// engine manifest（`engine/manifest.json`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineManifest {
    /// engine 版本（对齐 inkos package.json version，如 "1.7.2"）。
    pub engine_version: String,
    /// engine bundle（tarball）的 SHA256 hex；M3d updater 下载后据此校验完整性。
    pub engine_sha256: String,
    /// bootstrap 最低 node 版本（M3c，如 "22.0.0"）。
    pub node_min: String,
    /// 打包时间（ISO 8601，审计用）。
    pub built_at: String,
}

impl EngineManifest {
    /// 从 JSON 文件读 manifest。
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("读取 manifest 失败: {}", path.display()))?;
        let manifest: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("解析 manifest JSON 失败: {}", path.display()))?;
        Ok(manifest)
    }

    /// 原子写 manifest 到 JSON 文件（tempfile + persist，崩溃安全）。
    ///
    /// manifest 无敏感数据（无密钥），权限用默认（非 0600）；原子性仍保证（避免半写）。
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .context("manifest path 应有父目录")?;
        std::fs::create_dir_all(parent)
            .with_context(|| format!("创建 manifest 父目录失败: {}", parent.display()))?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)
            .context("创建 manifest 临时文件失败")?;
        let json = serde_json::to_string_pretty(self)
            .context("序列化 manifest 失败")?;
        use std::io::Write;
        writeln!(tmp, "{json}").context("写 manifest 临时文件失败")?;
        tmp.persist(path)
            .map_err(|e| anyhow::anyhow!("persist manifest 失败: {e}"))?;
        Ok(())
    }
}

/// 计算单文件 SHA256（hex 小写）。M3d updater 校验下载的 engine bundle 用。
pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)
        .with_context(|| format!("打开文件失败: {}", path.display()))?;
    let mut hasher = Sha256::new();
    // 64KiB 缓冲流式读取：避免大 bundle 一次性入内存（架构 §6 性能/0GC 定位）。
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).context("读取文件失败")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EngineManifest {
        EngineManifest {
            engine_version: "1.7.2".to_string(),
            engine_sha256: "abc123".to_string(),
            node_min: "22.0.0".to_string(),
            built_at: "2026-08-06T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn manifest_round_trip_via_write_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("manifest.json");
        let original = sample();
        original.write(&path).unwrap();
        let loaded = EngineManifest::read(&path).unwrap();
        assert_eq!(loaded, original);
    }

    #[test]
    fn read_missing_file_errors() {
        let path = std::path::Path::new("/nonexistent/manifest.json");
        assert!(EngineManifest::read(path).is_err());
    }

    #[test]
    fn read_invalid_json_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("manifest.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(EngineManifest::read(&path).is_err());
    }

    #[test]
    fn sha256_file_known_vector() {
        // SHA256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("blob.bin");
        std::fs::write(&path, b"abc").unwrap();
        let hash = sha256_file(&path).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_file_missing_errors() {
        assert!(sha256_file(std::path::Path::new("/nonexistent/blob")).is_err());
    }
}
