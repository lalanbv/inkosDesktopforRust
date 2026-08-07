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

        // 安全检查：确保目标路径不会逃逸沙箱。
        // 对新文件不能用 canonicalize（文件尚不存在），故手工解析路径组件：
        // 只接受 Normal/CurDir/ParentDir，拒绝 RootDir/Prefix 等绝对/跨盘组件。
        // 安全关键：写入必须用这里解析出的 `normalized`，而非 work_dir.join(path)
        // （后者保留原始 `..`/符号链接组件，可能落到沙箱外）。校验路径与写入路径
        // 必须是同一条路径——校验一条、写入另一条是经典的路径校验缺陷。
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

        // 确保解析后的路径仍在沙箱内（`..` 折叠后若回到 work_dir 之上则拒绝）。
        if !normalized.starts_with(&self.work_dir) {
            return Err(PluginError::PermissionDenied(
                "路径逃逸: 不在工作目录内".to_string(),
            ));
        }

        // 写入「已校验」的 normalized 路径（非原始 join 结果）。
        // 残留风险：work_dir 内预置的符号链接（如恶意插件 bundle 解压产物）仍可能
        // 将 normalized 解析到沙箱外——彻底闭环需在安装期拒绝符号链接条目
        // （见 docs/security-audit.md「已知残留」），此处保证字面逃逸被阻断。
        std::fs::write(&normalized, content).map_err(|e| {
            error!(
                plugin_id = %self.metadata.id,
                path = %normalized.display(),
                error = %e,
                "write_file: 写入失败"
            );
            PluginError::ExecutionFailed(format!("写入文件失败: {}", e))
        })?;

        tracing::info!(
            plugin_id = %self.metadata.id,
            path = %normalized.display(),
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
    /// 执行系统命令。
    ///
    /// # 可达性与安全契约
    /// - **当前不可达自不可信边界**：WASM 插件经 Host trait（read_file/write_file/
    ///   list_dir/http_get/log）调用，**不暴露 exec_command**（wit/inkos.wit 无此导入）。
    ///   故 WASM 插件无法执行系统命令——强隔离边界的任意执行面为零。
    /// - `Capability::SystemCommand` 是**全量信任**能力（无命令白名单，与 Network 的
    ///   `allowed_domains` 不对称）。一旦具备即可执行任意二进制 + 任意参数。
    /// - **未来接线约束**：若将本方法接入 WASI host imports 或 Tauri 命令，必须先引入
    ///   命令白名单（如 `Capability::SystemCommand { allowed_commands }`），否则等于
    ///   把任意代码执行暴露给不可信插件。当前仅在安装期 UX 告警（见 settings.html）。
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

// =====================================================================
// 网络能力白名单（Phase 6 最安全可靠）
// =====================================================================
// 进程隔离插件是独立进程，无法限制网络（OS 级沙箱超出范围）；WASM 插件经
// Host trait 调用本 http_get，域名白名单在此强制——插件无法绕过。

/// 从 URL 提取 host（小写，owned）。纯函数，便于单测。
/// `https://api.example.com/path?x=1` → `api.example.com`
pub fn extract_host(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = after_scheme.split('/').next()?.split(':').next()?.to_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// 校验 URL 的 host 是否在白名单。
/// - `*` 通配任意
/// - 精确匹配（大小写不敏感）
/// - 后缀匹配：allowed `example.com` 匹配 `api.example.com`（子域）
pub fn check_network_domain(url: &str, allowed: &[String]) -> bool {
    let host = match extract_host(url) {
        Some(h) => h,
        None => return false,
    };
    allowed.iter().any(|a| {
        let a = a.to_lowercase();
        a == "*" || a == host || host.ends_with(&format!(".{a}"))
    })
}

/// 是否为内网/保留 IP 字面量（SSRF 防护：拒直连 loopback/private/link-local/unspecified）。
/// 非 IP 字面量（域名）返回 false——域名解析到内网（DNS rebinding）为残留向量，
/// 需连接期 IP pinning，超出本层职责。
pub fn is_internal_ip(host: &str) -> bool {
    use std::net::IpAddr;
    use std::str::FromStr;
    let Ok(ip) = IpAddr::from_str(host) else {
        return false;
    };
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

/// 网络 API
impl HostContext {
    /// 返回声明的网络白名单（无 Network capability → None）
    fn network_allowed_domains(&self) -> Option<&[String]> {
        self.metadata.capabilities.iter().find_map(|cap| {
            if let Capability::Network { allowed_domains } = cap {
                Some(allowed_domains.as_slice())
            } else {
                None
            }
        })
    }

    /// HTTP GET（受域名白名单限制）。用 ureq 纯同步——避免 reqwest::blocking
    /// 在 tokio 上下文（WASM execute 经 async manager 调用）的嵌套 runtime panic。
    pub fn http_get(&self, url: &str) -> Result<String, PluginError> {
        let allowed = self.network_allowed_domains().ok_or_else(|| {
            warn!(plugin_id = %self.metadata.id, url = %url, "http_get: 缺少 network 权限");
            PluginError::PermissionDenied("缺少 network 权限".to_string())
        })?;

        if !check_network_domain(url, allowed) {
            warn!(plugin_id = %self.metadata.id, url = %url, "http_get: 域名不在白名单");
            return Err(PluginError::PermissionDenied(format!(
                "域名不在白名单: {url}"
            )));
        }

        // SSRF：拒直连内网/loopback/链路本地/未指定 IP（即便 "*" 全开放插件，
        // 也不应访问云元数据 169.254.169.254 / localhost / 私网）。域名→内网
        // （DNS rebinding）为残留向量，需连接期 IP pinning，超出本层。
        if let Some(host) = extract_host(url) {
            if is_internal_ip(&host) {
                warn!(plugin_id = %self.metadata.id, host = %host, "http_get: 拒内网 IP（SSRF）");
                return Err(PluginError::PermissionDenied(format!(
                    "拒访问内网/保留 IP: {host}"
                )));
            }
        }

        tracing::info!(plugin_id = %self.metadata.id, url = %url, "http_get: 允许");
        // SSRF 防护：禁用重定向跟随。允许域若 302 到内网/元数据 IP（如 169.254.169.254），
        // 默认 ureq 跟随且不复核域名 → 重定向 SSRF。禁用后插件拿到 3xx；其手动跟进会
        // 再经本 http_get 复核白名单。agent 每次新建（轻量，无连接池需求）。
        let agent = ureq::AgentBuilder::new().redirects(0).build();
        agent
            .get(url)
            .call()
            .map_err(|e| PluginError::ExecutionFailed(format!("HTTP 请求失败: {e}")))?
            .into_string()
            .map_err(|e| PluginError::ExecutionFailed(format!("读取响应失败: {e}")))
    }
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
    fn test_write_file_path_traversal() {
        // write_file 校验的是手工解析的 normalized 路径，写入也必须是它。
        // `..` 逃逸必须被拒（覆盖「校验路径≠写入路径」缺陷的回归）。
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();
        let metadata = PluginMetadata {
            id: "trav".to_string(),
            name: "Trav".to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            homepage: None,
            license: String::new(),
            abi_version: "1".to_string(),
            capabilities: vec![
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

        // 单层 ..、多层 ..、绝对路径（RootDir）都应被拒，且不产生任何文件。
        // 注："C:\\evil" 仅在 Windows 是 Prefix（拒绝）；unix 下反斜杠是合法文件名字符，
        // 解析为相对 Normal，不算逃逸——故不纳入跨平台拒绝集。
        for bad in ["../evil.txt", "a/../../evil.txt", "/etc/evil"] {
            let result = ctx.write_file(bad, "x");
            assert!(
                matches!(result, Err(PluginError::PermissionDenied(_))),
                "write_file({bad:?}) 应被拒绝，实际：{result:?}"
            );
        }
        // 合法路径仍可写入（确保修复未误伤正常用例）。
        assert!(ctx.write_file("ok.txt", "ok").is_ok());
        assert!(temp_path.join("ok.txt").exists());
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

    #[test]
    fn test_extract_host() {
        assert_eq!(
            extract_host("https://api.example.com/path?x=1").as_deref(),
            Some("api.example.com")
        );
        assert_eq!(extract_host("http://localhost:3000").as_deref(), Some("localhost"));
        assert_eq!(extract_host("example.com").as_deref(), Some("example.com"));
        assert_eq!(extract_host("https://API.COM").as_deref(), Some("api.com"));
        assert!(extract_host("").is_none());
    }

    #[test]
    fn test_check_network_domain_wildcard() {
        let allowed = vec!["*".to_string()];
        assert!(check_network_domain("https://anywhere.com", &allowed));
        assert!(check_network_domain("https://evil.test", &allowed));
    }

    #[test]
    fn test_check_network_domain_exact() {
        let allowed = vec!["api.github.com".to_string(), "registry.npmjs.org".to_string()];
        assert!(check_network_domain("https://api.github.com/repos", &allowed));
        assert!(check_network_domain("https://registry.npmjs.org/pkg", &allowed));
        // 不在白名单
        assert!(!check_network_domain("https://evil.com", &allowed));
        assert!(!check_network_domain("https://api.github.com.evil.com", &allowed));
    }

    #[test]
    fn test_check_network_domain_subdomain_suffix() {
        // allowed example.com 匹配子域 api.example.com（后缀匹配）
        let allowed = vec!["example.com".to_string()];
        assert!(check_network_domain("https://example.com", &allowed));
        assert!(check_network_domain("https://api.example.com", &allowed));
        assert!(!check_network_domain("https://notexample.com", &allowed));
        assert!(!check_network_domain("https://evil.com", &allowed));
    }

    #[test]
    fn test_http_get_without_network_capability() {
        let (ctx, _temp) = create_test_context(vec![]);
        let result = ctx.http_get("https://example.com");
        assert!(matches!(result, Err(PluginError::PermissionDenied(_))));
    }

    #[test]
    fn test_http_get_domain_not_in_whitelist() {
        let (ctx, _temp) =
            create_test_context(vec![Capability::Network {
                allowed_domains: vec!["allowed.com".to_string()],
            }]);
        // 白名单外域名 → PermissionDenied（不发请求）
        let result = ctx.http_get("https://evil.com");
        assert!(matches!(result, Err(PluginError::PermissionDenied(_))));
    }

    // 注：重定向 SSRF（redirects(0)）的 e2e 测试无法在此层跑——测试服务器只能用
    // 127.0.0.1，而下方内网 IP 拦截会挡掉它（http_get 在连接前就拒）。redirects(0)
    // 由代码显式构造 + 安全文档保证；内网 IP 拦截由下方纯函数 + http_get 测试覆盖。

    #[test]
    fn test_is_internal_ip_classification() {
        assert!(is_internal_ip("127.0.0.1"));
        assert!(is_internal_ip("169.254.169.254")); // 云元数据（link-local）
        assert!(is_internal_ip("10.0.0.1"));
        assert!(is_internal_ip("192.168.1.1"));
        assert!(is_internal_ip("::1")); // IPv6 loopback
        assert!(!is_internal_ip("8.8.8.8")); // 公网
        assert!(!is_internal_ip("example.com")); // 域名（非 IP 字面量）
    }

    #[test]
    fn test_http_get_blocks_internal_ip() {
        // 即便 "*" 全开放，也拒直连内网/保留 IP（SSRF 纵深）
        let (ctx, _temp) = create_test_context(vec![Capability::Network {
            allowed_domains: vec!["*".to_string()],
        }]);
        for internal in [
            "http://127.0.0.1/",
            "http://169.254.169.254/",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
        ] {
            let result = ctx.http_get(internal);
            assert!(
                matches!(result, Err(PluginError::PermissionDenied(_))),
                "{internal} 应被内网 IP 拦截: {:?}",
                result
            );
        }
    }
}
