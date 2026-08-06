//! Host API - 插件可调用的系统功能
//!
//! 设计原则：
//! - 每个 API 调用前强制权限检查
//! - 路径操作限制在声明的沙箱内
//! - 所有敏感操作写入审计日志

use super::types::{Capability, PluginError, PluginMetadata};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{error, warn};

/// Host API 上下文
#[derive(Clone)]
pub struct HostContext {
    /// 插件元数据（包含权限声明）
    pub(crate) metadata: PluginMetadata,

    /// 工作目录（插件沙箱根路径）
    pub(crate) work_dir: PathBuf,
}

impl HostContext {
    /// 创建新的 Host 上下文
    pub fn new(metadata: PluginMetadata, work_dir: PathBuf) -> Self {
        Self { metadata, work_dir }
    }

    /// 检查是否有指定权限
    fn has_capability(&self, cap: &Capability) -> bool {
        self.metadata.capabilities.contains(cap)
    }

    /// 检查是否有文件系统权限（路径匹配）
    fn has_filesystem_capability(&self, path: &Path) -> bool {
        // 规范化 work_dir 和 path 以比较真实路径
        let canonical_work_dir = self.work_dir.canonicalize().ok();
        let canonical_path = path.canonicalize().ok();

        self.metadata.capabilities.iter().any(|cap| {
            if let Capability::Filesystem { path: allowed_path } = cap {
                // 尝试规范化允许的路径
                if let Ok(canonical_allowed) = Path::new(allowed_path).canonicalize() {
                    if let (Some(ref cwd), Some(ref cp)) = (&canonical_work_dir, &canonical_path) {
                        // 比较规范化后的路径：允许的路径包含 work_dir 或者文件在允许路径下
                        return cwd.starts_with(&canonical_allowed) || cp.starts_with(&canonical_allowed);
                    }
                }
                // 回退到字符串比较
                path.starts_with(allowed_path)
            } else {
                false
            }
        })
    }

    /// 规范化路径（确保在沙箱内）
    fn normalize_path(&self, path: &str) -> Result<PathBuf, PluginError> {
        let full_path = self.work_dir.join(path);

        // 规范化路径（解析 .. 和软链接）
        let canonical = full_path
            .canonicalize()
            .map_err(|e| PluginError::PermissionDenied(format!("路径不存在或无法访问: {}", e)))?;

        // 规范化 work_dir（macOS /var 是 /private/var 的符号链接）
        let canonical_work_dir = self.work_dir
            .canonicalize()
            .map_err(|e| PluginError::PermissionDenied(format!("工作目录无法访问: {}", e)))?;

        // 确保路径仍在沙箱内
        if !canonical.starts_with(&canonical_work_dir) {
            return Err(PluginError::PermissionDenied(format!(
                "路径逃逸: {} 不在工作目录内",
                canonical.display()
            )));
        }

        Ok(canonical)
    }
}

/// 文件系统 API
impl HostContext {
    /// 读取文件
    pub fn read_file(&self, path: &str) -> Result<ReadFileResponse, PluginError> {
        // 权限检查
        if !self.has_capability(&Capability::ReadProject) {
            warn!(
                plugin_id = %self.metadata.id,
                path = %path,
                "read_file: 缺少 read_project 权限"
            );
            return Err(PluginError::PermissionDenied(
                "缺少 read_project 权限".to_string(),
            ));
        }

        // 路径验证
        let full_path = self.normalize_path(path)?;

        // 额外的文件系统权限检查
        if !self.has_filesystem_capability(&full_path) {
            warn!(
                plugin_id = %self.metadata.id,
                path = %full_path.display(),
                "read_file: 路径不在允许列表中"
            );
            return Err(PluginError::PermissionDenied(format!(
                "路径 {} 不在允许列表中",
                full_path.display()
            )));
        }

        // 读取文件
        let content = std::fs::read_to_string(&full_path).map_err(|e| {
            error!(
                plugin_id = %self.metadata.id,
                path = %full_path.display(),
                error = %e,
                "read_file: 读取失败"
            );
            PluginError::ExecutionFailed(format!("读取文件失败: {}", e))
        })?;

        tracing::debug!(
            plugin_id = %self.metadata.id,
            path = %full_path.display(),
            size = content.len(),
            "read_file: 成功"
        );

        Ok(ReadFileResponse { content })
    }

    /// 写入文件
    pub fn write_file(&self, path: &str, content: &str) -> Result<WriteFileResponse, PluginError> {
        // 权限检查
        if !self.has_capability(&Capability::WriteProject) {
            warn!(
                plugin_id = %self.metadata.id,
                path = %path,
                "write_file: 缺少 write_project 权限"
            );
            return Err(PluginError::PermissionDenied(
                "缺少 write_project 权限".to_string(),
            ));
        }

        // 构建完整路径
        let full_path = self.work_dir.join(path);

        // 安全检查：确保目标路径不会逃逸沙箱
        // 对于新文件，我们需要解析路径组件而不是 canonicalize（文件还不存在）
        let mut normalized = self.work_dir.clone();
        for component in Path::new(path).components() {
            match component {
                std::path::Component::Normal(part) => normalized.push(part),
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                std::path::Component::CurDir => {}
                _ => {
                    return Err(PluginError::PermissionDenied(
                        "路径包含非法组件".to_string(),
                    ));
                }
            }
        }

        // 确保规范化后的路径仍在沙箱内
        if !normalized.starts_with(&self.work_dir) {
            return Err(PluginError::PermissionDenied(
                "路径逃逸: 不在工作目录内".to_string(),
            ));
        }

        // 写入文件
        std::fs::write(&full_path, content).map_err(|e| {
            error!(
                plugin_id = %self.metadata.id,
                path = %full_path.display(),
                error = %e,
                "write_file: 写入失败"
            );
            PluginError::ExecutionFailed(format!("写入文件失败: {}", e))
        })?;

        tracing::info!(
            plugin_id = %self.metadata.id,
            path = %full_path.display(),
            size = content.len(),
            "write_file: 成功"
        );

        Ok(WriteFileResponse { success: true })
    }

    /// 列出目录
    pub fn list_dir(&self, path: &str) -> Result<ListDirResponse, PluginError> {
        // 权限检查
        if !self.has_capability(&Capability::ReadProject) {
            return Err(PluginError::PermissionDenied(
                "缺少 read_project 权限".to_string(),
            ));
        }

        // 路径验证
        let full_path = self.normalize_path(path)?;

        // 读取目录
        let entries: Vec<String> = std::fs::read_dir(&full_path)
            .map_err(|e| PluginError::ExecutionFailed(format!("读取目录失败: {}", e)))?
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    e.file_name().to_str().map(|s| s.to_string())
                })
            })
            .collect();

        Ok(ListDirResponse { entries })
    }
}

/// 系统 API
impl HostContext {
    /// 执行系统命令
    pub fn exec_command(&self, command: &str, args: &[String]) -> Result<ExecCommandResponse, PluginError> {
        // 权限检查
        if !self.has_capability(&Capability::SystemCommand) {
            warn!(
                plugin_id = %self.metadata.id,
                command = %command,
                "exec_command: 缺少 system_command 权限"
            );
            return Err(PluginError::PermissionDenied(
                "缺少 system_command 权限".to_string(),
            ));
        }

        // 审计日志
        tracing::warn!(
            plugin_id = %self.metadata.id,
            command = %command,
            args = ?args,
            "exec_command: 执行系统命令"
        );

        // 执行命令
        let output = std::process::Command::new(command)
            .args(args)
            .current_dir(&self.work_dir)
            .output()
            .map_err(|e| PluginError::ExecutionFailed(format!("执行命令失败: {}", e)))?;

        Ok(ExecCommandResponse {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

// === 请求/响应类型 ===

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFileResponse {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteFileResponse {
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDirResponse {
    pub entries: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecCommandResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_context(capabilities: Vec<Capability>) -> (HostContext, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let metadata = PluginMetadata {
            id: "test-plugin".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Test".to_string(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities,
            entrypoint: "plugin.wasm".to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        };

        let ctx = HostContext::new(metadata, temp_dir.path().to_path_buf());
        (ctx, temp_dir)
    }

    #[test]
    fn test_read_file_without_permission() {
        let (ctx, _temp) = create_test_context(vec![]);
        let result = ctx.read_file("test.txt");
        assert!(matches!(result, Err(PluginError::PermissionDenied(_))));
    }

    #[test]
    fn test_read_file_path_traversal() {
        let (ctx, _temp) = create_test_context(vec![Capability::ReadProject]);
        let result = ctx.read_file("../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_file_without_permission() {
        let (ctx, _temp) = create_test_context(vec![]);
        let result = ctx.write_file("test.txt", "content");
        assert!(matches!(result, Err(PluginError::PermissionDenied(_))));
    }

    #[test]
    fn test_read_write_file_success() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();

        let metadata = PluginMetadata {
            id: "test-plugin".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Test".to_string(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities: vec![
                Capability::ReadProject,
                Capability::WriteProject,
                Capability::Filesystem {
                    path: temp_path.to_string_lossy().to_string(),
                },
            ],
            entrypoint: "plugin.wasm".to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        };

        let ctx = HostContext::new(metadata, temp_path.clone());

        // 写入文件
        let write_result = ctx.write_file("test.txt", "Hello, World!");
        assert!(write_result.is_ok(), "write failed: {:?}", write_result);

        // 读取文件
        let read_result = ctx.read_file("test.txt");
        assert!(read_result.is_ok(), "read failed: {:?}", read_result);
        assert_eq!(read_result.unwrap().content, "Hello, World!");

        // 验证文件确实存在
        assert!(temp_path.join("test.txt").exists());
    }

    #[test]
    fn test_exec_command_without_permission() {
        let (ctx, _temp) = create_test_context(vec![]);
        let result = ctx.exec_command("echo", &["test".to_string()]);
        assert!(matches!(result, Err(PluginError::PermissionDenied(_))));
    }

    #[test]
    fn test_list_dir() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();

        let metadata = PluginMetadata {
            id: "test-plugin".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Test".to_string(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities: vec![
                Capability::ReadProject,
                Capability::WriteProject,
                Capability::Filesystem {
                    path: temp_path.to_string_lossy().to_string(),
                },
            ],
            entrypoint: "plugin.wasm".to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        };

        let ctx = HostContext::new(metadata, temp_path.clone());

        // 创建测试文件
        ctx.write_file("file1.txt", "content1").unwrap();
        ctx.write_file("file2.txt", "content2").unwrap();

        // 列出目录
        let result = ctx.list_dir(".").unwrap();
        assert_eq!(result.entries.len(), 2);
        assert!(result.entries.contains(&"file1.txt".to_string()));
        assert!(result.entries.contains(&"file2.txt".to_string()));
    }
}
