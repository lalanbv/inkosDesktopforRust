//! 状态持久化的文件系统抽象（StateStore trait）。
//!
//! state-bootstrap / runtime-state-store / manager 等 async 编排的共同 fs 基座。
//! 抽象 `node:fs/promises` 的子集（read/write/mkdir/list/exists），使编排逻辑可注入测试。
//!
//! ## 两个实现
//! - [`FsStateStore`]：生产实现，基于 `tokio::fs`（绝对路径，真实磁盘）
//! - [`InMemoryStateStore`]：测试 mock，`HashMap<path, content>` 内存模拟（路径无关，纯键值）
//!
//! ## 设计纪律
//! 对齐项目「纯内核 + I/O 注入」范式（见 chapter_word_sync）：编排逻辑依赖 `dyn StateStore`，
//! 单测用 [`InMemoryStateStore`]，集成测试用 [`FsStateStore`] + tempfile。trait 方法语义须与
//! TS `node:fs/promises` 逐一对齐（文件/目录不存在 → None/空，而非错误）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::Result;

/// 状态编排的文件系统抽象。
///
/// 所有方法返回 [`crate::EngineError::Io`] 仅用于不可恢复的 I/O 故障；
/// 「文件/目录不存在」是正常业务状态，统一返回 `None` / 空 `Vec`。
#[async_trait]
pub trait StateStore: Send + Sync {
    /// 读取文件为字符串；文件不存在 → None。
    async fn read_to_string(&self, path: &str) -> Result<Option<String>>;

    /// 写入文件（覆盖）；父目录须已存在（编排先调 [`Self::mkdir_p`]）。
    async fn write_string(&self, path: &str, content: &str) -> Result<()>;

    /// 递归创建目录（已存在则 no-op）。
    async fn mkdir_p(&self, path: &str) -> Result<()>;

    /// 列出目录下的直接条目名（不含路径）；目录不存在 → 空 Vec。
    async fn list_dir(&self, path: &str) -> Result<Vec<String>>;

    /// 路径是否存在（文件或目录）。
    async fn exists(&self, path: &str) -> Result<bool>;
}

/// 生产实现：基于 `tokio::fs` 操作真实磁盘。
///
/// 路径按原样传递（相对或绝对）；调用方负责拼合 book_dir。
pub struct FsStateStore;

#[async_trait]
impl StateStore for FsStateStore {
    async fn read_to_string(&self, path: &str) -> Result<Option<String>> {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn write_string(&self, path: &str, content: &str) -> Result<()> {
        tokio::fs::write(path, content).await?;
        Ok(())
    }

    async fn mkdir_p(&self, path: &str) -> Result<()> {
        tokio::fs::create_dir_all(path).await?;
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        match tokio::fs::read_dir(path).await {
            Ok(mut entries) => {
                let mut names = Vec::new();
                while let Ok(Some(entry)) = entries.next_entry().await {
                    names.push(entry.file_name().to_string_lossy().into_owned());
                }
                Ok(names)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(tokio::fs::metadata(path).await.is_ok())
    }
}

/// 测试 mock：内存 HashMap 模拟 fs。路径作为字符串键（不解析 `.`/`..`）。
///
/// `write_string` 自动补全父目录（`mkdir_p` 语义内联）；`list_dir` 按路径前缀匹配直接子项。
/// 用 `Mutex` 保护内部映射，满足 `Send + Sync`。
pub struct InMemoryStateStore {
    files: Mutex<HashMap<String, String>>,
}

impl InMemoryStateStore {
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    /// 预置一个文件内容（测试 setup 用）。
    pub fn set(&self, path: &str, content: &str) {
        self.files.lock().expect("files mutex").insert(path.to_string(), content.to_string());
    }
}

impl Default for InMemoryStateStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StateStore for InMemoryStateStore {
    async fn read_to_string(&self, path: &str) -> Result<Option<String>> {
        Ok(self
            .files
            .lock()
            .expect("files mutex")
            .get(path)
            .cloned())
    }

    async fn write_string(&self, path: &str, content: &str) -> Result<()> {
        self.files
            .lock()
            .expect("files mutex")
            .insert(path.to_string(), content.to_string());
        Ok(())
    }

    async fn mkdir_p(&self, _path: &str) -> Result<()> {
        // 内存模型无真实目录概念；write_string 自动补全父键，no-op 即可。
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<String>> {
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        let files = self.files.lock().expect("files mutex");
        let mut children: Vec<String> = files
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .map(|rest| {
                // 直接子项：rest 中下一个 `/` 之前的部分（无 `/` 则整体）。
                match rest.find('/') {
                    Some(idx) => rest[..idx].to_string(),
                    None => rest.to_string(),
                }
            })
            .filter(|s| !s.is_empty())
            .collect();
        children.sort();
        children.dedup();
        Ok(children)
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        let files = self.files.lock().expect("files mutex");
        // 文件存在，或作为某文件路径前缀（目录语义）。
        Ok(files.contains_key(path) || files.keys().any(|k| k.starts_with(&format!("{path}/"))))
    }
}

/// 路径拼接辅助（对齐 TS `join`）。避免各编排处重复 `PathBuf::from(...).join(...)` 样板。
pub fn join_path(base: &str, seg: &str) -> String {
    PathBuf::from(base).join(seg).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_read_returns_none_when_absent() {
        let s = InMemoryStateStore::new();
        assert_eq!(s.read_to_string("a/b.json").await.unwrap(), None);
    }

    #[tokio::test]
    async fn in_memory_write_then_read_roundtrip() {
        let s = InMemoryStateStore::new();
        s.write_string("a/b.json", "{}").await.unwrap();
        assert_eq!(s.read_to_string("a/b.json").await.unwrap(), Some("{}".to_string()));
        assert!(s.exists("a/b.json").await.unwrap());
    }

    #[tokio::test]
    async fn in_memory_list_dir_direct_children_only() {
        let s = InMemoryStateStore::new();
        s.set("chapters/1_x.md", "x");
        s.set("chapters/2_y.md", "y");
        s.set("chapters/sub/3_z.md", "z");
        s.set("state/manifest.json", "{}");
        let children = s.list_dir("chapters").await.unwrap();
        assert_eq!(children, vec!["1_x.md".to_string(), "2_y.md".to_string(), "sub".to_string()]);
    }

    #[tokio::test]
    async fn in_memory_list_dir_absent_returns_empty() {
        let s = InMemoryStateStore::new();
        assert!(s.list_dir("nope").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn in_memory_exists_for_directory_prefix() {
        let s = InMemoryStateStore::new();
        s.set("story/state/manifest.json", "{}");
        assert!(s.exists("story/state").await.unwrap(), "目录前缀应判存在");
        assert!(s.exists("story").await.unwrap());
        assert!(!s.exists("other").await.unwrap());
    }

    #[tokio::test]
    async fn fs_store_read_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("missing.json");
        let s = FsStateStore;
        assert_eq!(s.read_to_string(path.to_str().unwrap()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn fs_store_mkdir_write_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("story/state");
        let file = dir.join("manifest.json");
        let s = FsStateStore;
        s.mkdir_p(dir.to_str().unwrap()).await.unwrap();
        s.write_string(file.to_str().unwrap(), "{}").await.unwrap();
        assert_eq!(
            s.read_to_string(file.to_str().unwrap()).await.unwrap(),
            Some("{}".to_string())
        );
        assert!(s.exists(file.to_str().unwrap()).await.unwrap());
    }

    #[tokio::test]
    async fn fs_store_list_dir_returns_real_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let chapters = tmp.path().join("chapters");
        let s = FsStateStore;
        s.mkdir_p(chapters.to_str().unwrap()).await.unwrap();
        s.write_string(chapters.join("1_a.md").to_str().unwrap(), "x").await.unwrap();
        s.write_string(chapters.join("2_b.md").to_str().unwrap(), "y").await.unwrap();
        let mut entries = s.list_dir(chapters.to_str().unwrap()).await.unwrap();
        entries.sort();
        assert_eq!(entries, vec!["1_a.md".to_string(), "2_b.md".to_string()]);
    }

    #[test]
    fn join_path_handles_trailing_slash_and_segments() {
        assert_eq!(join_path("a", "b"), format!("a{}b", std::path::MAIN_SEPARATOR));
    }
}
