//! 跨模块共享工具。
//!
//! M3b 提取：`atomic_write_0600` 原为 `secrets/jsonio.rs` 私有，现 projects.json 也需
//! 同语义原子写（权限 0600 + 崩溃安全），提取为共享以 DRY（审计重视此模式）。

use std::path::Path;

use anyhow::Context;

/// 原子写文件（Unix 权限 0600）：`NamedTempFile`（创建即 0600）+ `persist` 原子 rename。
///
/// - 0600（Unix）：消除 `fs::write` 默认权限（受 umask 影响，通常 0644）的"短暂可读
///   窗口"——内容从未以非 0600 状态落盘（secrets.json / projects.json 等）。
/// - `persist`：同 filesystem 原子 rename；跨 fs 会 fallback 到非原子拷贝（本场景
///   目标文件与其 tmp 同目录，不会跨 fs）。Windows 无 0600（NTFS ACL 另论，跳过）。
/// - 失败语义：`tempfile_in` / `write_all` / `flush` / `persist` 任一失败 → 显式 `Err`；
///   `NamedTempFile` drop 自动清理未 persist 的 tmp（无垃圾残留）。
pub fn atomic_write_0600(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("atomic_write: path={} 无父目录", path.display()))?;

    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(PermissionsExt::from_mode(0o600));
    }
    let mut tmp = builder
        .tempfile_in(parent)
        .with_context(|| format!("创建 NamedTempFile 失败: {}", parent.display()))?;

    use std::io::Write;
    tmp.write_all(data)
        .with_context(|| format!("write tmp 失败: {}", tmp.path().display()))?;
    tmp.flush()
        .with_context(|| format!("flush tmp 失败: {}", tmp.path().display()))?;
    // M4 审计修复：fsync 到磁盘再 persist（flush 只到 OS page cache；断电可致半写/空文件）。
    // 对 secrets/projects（敏感/用户数据）值得 ms 级延迟换持久性。
    tmp.as_file()
        .sync_all()
        .with_context(|| format!("fsync tmp 失败: {}", tmp.path().display()))?;

    if let Err(e) = tmp.persist(path) {
        return Err(anyhow::anyhow!(
            "persist tmp -> {} 失败: {}",
            path.display(),
            e.error
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn atomic_write_0600_produces_0600_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f.bin");
        atomic_write_0600(&path, b"hello").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn atomic_write_0600_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f.bin");
        atomic_write_0600(&path, b"old").unwrap();
        atomic_write_0600(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
}
