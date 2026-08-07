//! Host API - 插件可调用的系统功能
//!
//! 设计原则：
//! - 每个 API 调用前强制权限检查
//! - 路径操作限制在声明的沙箱内
//! - 所有敏感操作写入审计日志

use super::types::{Capability, PluginError, PluginMetadata};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{error, warn};

/// Host API 上下文
///
/// `metadata` 用 `Arc` 共享：WASM 执行热路径每次调用 `create_store` 都会
/// `clone()` 一份 HostContext 给独立 Store（状态隔离）。metadata 含 8+ String +
/// capabilities Vec，clone 成本高；改 Arc 后 clone 退化为单次原子递增（~0 分配）。
/// metadata 构造后不可变（仅权限校验 + 日志读取），共享语义安全。`work_dir` 保持
/// owned PathBuf——仅 1 次分配且多处需 owned 路径（join/canonicalize/starts_with），
/// Arc 化反增 Deref/AsRef 摩擦，收益不抵成本。
#[derive(Clone)]
pub struct HostContext {
    /// 插件元数据（包含权限声明）—— Arc 共享，clone 廉价
    pub(crate) metadata: Arc<PluginMetadata>,

    /// 工作目录（插件沙箱根路径）
    pub(crate) work_dir: PathBuf,
}

impl HostContext {
    /// 创建新的 Host 上下文（metadata 包装进 Arc 供后续廉价 clone）
    pub fn new(metadata: PluginMetadata, work_dir: PathBuf) -> Self {
        Self {
            metadata: Arc::new(metadata),
            work_dir,
        }
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
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                path = %path,
                action = "read_file",
                "能力拒绝: 缺少 read_project 权限"
            );
            return Err(PluginError::PermissionDenied(
                "缺少 read_project 权限".to_string(),
            ));
        }

        // 路径验证
        let full_path = self.normalize_path(path)?;

        // 文件大小守卫（在 filesystem 能力检查之前，fail-fast 避免不必要的权限查询）。
        // metadata().len() 为系统调用（无 IO），代价可忽略。
        const MAX_READ_FILE_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB
        let file_size = std::fs::metadata(&full_path)
            .map(|m| m.len())
            .unwrap_or(0);
        if file_size > MAX_READ_FILE_BYTES {
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                path = %full_path.display(),
                file_size = file_size,
                limit = MAX_READ_FILE_BYTES,
                action = "read_file",
                "文件过大拒绝: 超过单次读取上限"
            );
            return Err(PluginError::PermissionDenied(format!(
                "文件 {} 过大（{file_size} 字节）超过读取上限 {MAX_READ_FILE_BYTES}",
                full_path.display()
            )));
        }

        // 额外的文件系统权限检查
        if !self.has_filesystem_capability(&full_path) {
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                path = %full_path.display(),
                action = "read_file",
                "路径拒绝: 不在 filesystem 白名单"
            );
            return Err(PluginError::PermissionDenied(format!(
                "路径 {} 不在允许列表中",
                full_path.display()
            )));
        }

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
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                path = %path,
                action = "write_file",
                "能力拒绝: 缺少 write_project 权限"
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
                    warn!(
                        target: "inkos.plugin.security",
                        plugin_id = %self.metadata.id,
                        path = %path,
                        action = "write_file",
                        "路径拒绝: 含非法路径组件（绝对路径/跨盘前缀）"
                    );
                    return Err(PluginError::PermissionDenied(
                        "路径包含非法组件".to_string(),
                    ));
                }
            }
        }

        // 确保解析后的路径仍在沙箱内（`..` 折叠后若回到 work_dir 之上则拒绝）。
        if !normalized.starts_with(&self.work_dir) {
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                path = %path,
                normalized = %normalized.display(),
                action = "write_file",
                "路径逃逸: 规范化后超出工作目录"
            );
            return Err(PluginError::PermissionDenied(
                "路径逃逸: 不在工作目录内".to_string(),
            ));
        }

        // 写入内容大小守卫（防插件写入超大文件耗尽磁盘）。
        const MAX_WRITE_FILE_BYTES: usize = 8 * 1024 * 1024; // 8 MiB
        if content.len() > MAX_WRITE_FILE_BYTES {
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                path = %normalized.display(),
                content_len = content.len(),
                limit = MAX_WRITE_FILE_BYTES,
                action = "write_file",
                "写入内容过大拒绝: 超过单次写入上限"
            );
            return Err(PluginError::PermissionDenied(format!(
                "写入内容 {} 字节超过上限 {MAX_WRITE_FILE_BYTES}",
                content.len()
            )));
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
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                path = %path,
                action = "list_dir",
                "能力拒绝: 缺少 read_project 权限"
            );
            return Err(PluginError::PermissionDenied(
                "缺少 read_project 权限".to_string(),
            ));
        }

        // 路径验证
        let full_path = self.normalize_path(path)?;

        // 读取目录：条目数上限，防海量目录项 OOM（每条 ~String alloc）
        const MAX_DIR_ENTRIES: usize = 4096;
        let entries: Vec<String> = std::fs::read_dir(&full_path)
            .map_err(|e| PluginError::ExecutionFailed(format!("读取目录失败: {}", e)))?
            .take(MAX_DIR_ENTRIES + 1) // +1 用于检测超限
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    e.file_name().to_str().map(|s| s.to_string())
                })
            })
            .collect();
        if entries.len() > MAX_DIR_ENTRIES {
            return Err(PluginError::ExecutionFailed(format!(
                "目录条目数超过上限 {MAX_DIR_ENTRIES}（防 OOM）"
            )));
        }

        Ok(ListDirResponse { entries })
    }
}

/// 系统 API
impl HostContext {
    /// `command` 是否在白名单内（`SystemCommand { allowed_commands }` 校验）。
    /// 空白名单 → false（fail-closed）。纯函数，便于单测。
    fn can_exec(&self, command: &str) -> bool {
        self.metadata.capabilities.iter().any(|cap| match cap {
            Capability::SystemCommand { allowed_commands } => {
                allowed_commands.iter().any(|c| c == command)
            }
            _ => false,
        })
    }

    /// 执行系统命令。
    ///
    /// # 可达性与安全契约
    /// - **当前不可达自不可信边界**：WASM 插件经 Host trait（read_file/write_file/
    ///   list_dir/http_get/log）调用，**不暴露 exec_command**（wit/inkos.wit 无此导入）。
    ///   故 WASM 插件无法执行系统命令——强隔离边界的任意执行面为零。
    /// - `Capability::SystemCommand { allowed_commands }` 现为**命令白名单**能力
    ///   （对称 Network 的 `allowed_domains`）：仅 `allowed_commands` 内的裸命令名可执行。
    ///   bare `system_command` = 空白名单（fail-closed，无命令可执行）。命令名经
    ///   `is_safe_command_name` 校验（拒路径/shell 元字符），白名单本身不成注入向量。
    /// - **接线就绪**：白名单已就位，若将本方法接入 WASI host imports / Tauri 命令，
    ///   任意代码执行面已被收敛到插件声明的命令集（仍须逐命令审计其参数语义）。
    pub fn exec_command(&self, command: &str, args: &[String]) -> Result<ExecCommandResponse, PluginError> {
        // 命令白名单检查：须声明 SystemCommand 能力 且 command ∈ allowed_commands。
        if !self.can_exec(command) {
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                command = %command,
                action = "exec_command",
                "能力拒绝: 命令不在 system_command.allowed_commands 白名单"
            );
            return Err(PluginError::PermissionDenied(
                "缺少 system_command 权限或命令不在白名单".to_string(),
            ));
        }

        // 审计日志：exec_command 成功执行也要记录（高危操作，全程可追溯）
        tracing::warn!(
            target: "inkos.plugin.security",
            plugin_id = %self.metadata.id,
            command = %command,
            args = ?args,
            "exec_command: 执行系统命令（已授权）"
        );

        // 参数安全校验：NUL 字节 → OS 截断参数（行为未定义）；过长参数 → 防 ARG_MAX 超限。
        // `Command::arg` 不使用 shell，无 shell injection 风险；
        // 但 NUL 字节仍可绕过日志截断引发误判，过长参数可触发 E2BIG。
        const MAX_ARG_LEN: usize = 4096;
        const MAX_ARG_COUNT: usize = 64;
        if args.len() > MAX_ARG_COUNT {
            return Err(PluginError::ExecutionFailed(format!(
                "参数数量 {} 超过上限 {MAX_ARG_COUNT}（防 ARG_MAX 耗尽）",
                args.len()
            )));
        }
        for (i, arg) in args.iter().enumerate() {
            if arg.contains('\0') {
                return Err(PluginError::ExecutionFailed(format!(
                    "参数 #{i} 含 NUL 字节（禁止）"
                )));
            }
            if arg.len() > MAX_ARG_LEN {
                return Err(PluginError::ExecutionFailed(format!(
                    "参数 #{i} 超过最大长度 {MAX_ARG_LEN}（实际 {}）",
                    arg.len()
                )));
            }
        }

        // 执行命令
        let output = std::process::Command::new(command)
            .args(args)
            .current_dir(&self.work_dir)
            .output()
            .map_err(|e| PluginError::ExecutionFailed(format!("执行命令失败: {}", e)))?;

        // 输出大小守卫：防进程输出 GB 级数据 OOM 宿主。超限时截断前 N 字节，
        // 不报错（允许插件处理截断输出）；截断信息在 stderr 末尾追加标记。
        const MAX_EXEC_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MiB per stream
        let truncate = |bytes: &[u8]| -> String {
            if bytes.len() > MAX_EXEC_OUTPUT_BYTES {
                let mut s = String::from_utf8_lossy(&bytes[..MAX_EXEC_OUTPUT_BYTES]).to_string();
                s.push_str("\n[输出截断: 超过 1MiB 上限]");
                s
            } else {
                String::from_utf8_lossy(bytes).to_string()
            }
        };

        Ok(ExecCommandResponse {
            stdout: truncate(&output.stdout),
            stderr: truncate(&output.stderr),
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
/// 正确处理 userinfo（`user:pass@host`）、端口、方括号 IPv6（`[::1]:443`）——
/// 防 userinfo 混淆（`https://allowed.com@127.0.0.1` 的真实 host 是 127.0.0.1）与
/// 方括号 IPv6 解析（`[` 非 IP 致 is_internal_ip 层失效），恢复 SSRF 2 层纵深。
/// `https://api.example.com/path?x=1` → `api.example.com`
pub fn extract_host(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    // authority 结束于首个 '/' '?' '#'（path/query/fragment 起始）
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    // 剥 userinfo：取最后一个 '@' 之后（`user:pass@host:port` → `host:port`）
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    // 剥端口 / 方括号 IPv6
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next()? // [::1]:443 → ::1
    } else {
        host_port
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(host_port) // host:port → host
    };
    let host = host.to_lowercase();
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
/// 非 IP 字面量（域名）返回 false——本层仅拦截直接 IP 字面量（含 IPv4-mapped/
/// 6to4/NAT64 包装）。域名解析到内网的 DNS rebinding 由 `ssrf_resolve` 连接期
/// IP pinning 缓解（见 `http_get`），本层为该防护的纵深一层，非残留。
pub fn is_internal_ip(host: &str) -> bool {
    use std::net::IpAddr;
    use std::str::FromStr;
    let Ok(ip) = IpAddr::from_str(host) else {
        return false;
    };
    is_internal_ip_addr(&ip)
}

/// `is_internal_ip` 的核心（已解析 IpAddr）。处理 IPv4-mapped IPv6
/// （`::ffff:127.0.0.1`）——映射的 IPv4 经 v4 判定，防用 mapped 形式绕过。
fn is_internal_ip_addr(ip: &std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            // 嵌入 IPv4 的解封装形式（命中即查嵌入 v4 是否内网）：
            //   - mapped     (::ffff:a.b.c.d)        —— to_ipv4_mapped
            //   - compatible (::a.b.c.d，RFC 4291 弃用，segs[0..6] 全零)
            //   - 6to4       (2002::/16，封装在 segs[1..3])
            //   - NAT64      (64:ff9b::/96，封装在 segs[6..8])
            //
            // **OR 结构（非提前 return）**：解封装判定与 v6 通用判定并联——确保 ::1
            // （解封装为 0.0.0.1，v4 非内网）/ ::（0.0.0.0）等特殊地址仍被 is_loopback /
            // is_unspecified 兜底，不因解封装误判而漏网（防回归）。
            let segs = v6.segments();
            let embedded_internal = v6
                .to_ipv4_mapped()
                .or_else(|| {
                    // IPv4-compatible：segs[0..6] 全零（排除 mapped 的 segs[5]=0xffff）
                    if segs[0..6].iter().all(|s| *s == 0) {
                        Some(std::net::Ipv4Addr::new(
                            (segs[6] >> 8) as u8,
                            segs[6] as u8,
                            (segs[7] >> 8) as u8,
                            segs[7] as u8,
                        ))
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    // 6to4（2002::/16）
                    if segs[0] == 0x2002 {
                        Some(std::net::Ipv4Addr::new(
                            (segs[1] >> 8) as u8,
                            segs[1] as u8,
                            (segs[2] >> 8) as u8,
                            segs[2] as u8,
                        ))
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    // NAT64 well-known（64:ff9b::/96）
                    if segs[0] == 0x0064 && segs[1] == 0xff9b {
                        Some(std::net::Ipv4Addr::new(
                            (segs[6] >> 8) as u8,
                            segs[6] as u8,
                            (segs[7] >> 8) as u8,
                            segs[7] as u8,
                        ))
                    } else {
                        None
                    }
                })
                .map(|v4| is_internal_ip_addr(&IpAddr::V4(v4)))
                .unwrap_or(false);
            embedded_internal
                || v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

/// 从解析结果中过滤掉内网/保留 IP。全为内网 → Err（SSRF 拒绝）。纯函数，可单测。
pub fn filter_public_addrs(
    addrs: Vec<std::net::SocketAddr>,
) -> std::io::Result<Vec<std::net::SocketAddr>> {
    let public: Vec<_> = addrs
        .into_iter()
        .filter(|sa| !is_internal_ip_addr(&sa.ip()))
        .collect();
    if public.is_empty() {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "解析到的 IP 均为内网/保留地址（SSRF 防护：DNS rebinding 拒绝）",
        ))
    } else {
        Ok(public)
    }
}

/// ureq 自定义 Resolver：解析 netloc → 过滤内网 IP → 仅返公网 IP。
/// ureq 用本 resolver 返回的 IP 连接（IP pinning），关闭「解析-连接」DNS rebinding 窗口。
fn ssrf_resolve(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    let addrs: Vec<std::net::SocketAddr> =
        std::net::ToSocketAddrs::to_socket_addrs(netloc)?.collect();
    filter_public_addrs(addrs)
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
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                url = %url,
                action = "http_get",
                "能力拒绝: 缺少 network 权限"
            );
            PluginError::PermissionDenied("缺少 network 权限".to_string())
        })?;

        if !check_network_domain(url, allowed) {
            warn!(
                target: "inkos.plugin.security",
                plugin_id = %self.metadata.id,
                url = %url,
                action = "http_get",
                "域名拒绝: 不在 network.allowed_domains 白名单"
            );
            return Err(PluginError::PermissionDenied(format!(
                "域名不在白名单: {url}"
            )));
        }

        // SSRF 纵深一层：拒直连内网/loopback/链路本地/未指定 IP 字面量（即便 "*"
        // 全开放插件，也不应访问云元数据 169.254.169.254 / localhost / 私网）。
        // 域名解析到内网的 DNS rebinding 由下方 ssrf_resolve 连接期 IP pinning 缓解——
        // 本层仅拦字面量形式，与 resolver 层互为纵深（非残留风险）。
        if let Some(host) = extract_host(url) {
            if is_internal_ip(&host) {
                warn!(
                    target: "inkos.plugin.security",
                    plugin_id = %self.metadata.id,
                    host = %host,
                    url = %url,
                    action = "http_get",
                    "SSRF 拒绝: 内网/保留 IP 字面量"
                );
                return Err(PluginError::PermissionDenied(format!(
                    "拒访问内网/保留 IP: {host}"
                )));
            }
        }

        tracing::info!(plugin_id = %self.metadata.id, url = %url, "http_get: 允许");
        // SSRF 防护：禁重定向 + DNS rebinding 防护（自定义 resolver 过滤内网 IP，
        // ureq 用过滤后的公网 IP 连接 = IP pinning，关闭解析-连接 TOCTOU）。
        const MAX_HTTP_RESPONSE_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .resolver(ssrf_resolve)
            .build();
        let resp = agent
            .get(url)
            .call()
            .map_err(|e| PluginError::ExecutionFailed(format!("HTTP 请求失败: {e}")))?;
        // 响应体大小守卫：ureq into_string() 不限制大小，超大响应 OOM 宿主。
        // 用 read_to_string 配合 BufReader 限制字节数读取。
        use std::io::Read;
        let mut body = String::new();
        resp.into_reader()
            .take(MAX_HTTP_RESPONSE_BYTES)
            .read_to_string(&mut body)
            .map_err(|e| PluginError::ExecutionFailed(format!("读取响应失败: {e}")))?;
        Ok(body)
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
    fn test_exec_command_whitelist_denies_non_listed() {
        // 声明 system_command:echo 但执行 ls → 白名单拒绝（ls 不执行）。
        let (ctx, _temp) = create_test_context(vec![Capability::SystemCommand {
            allowed_commands: vec!["echo".to_string()],
        }]);
        let result = ctx.exec_command("ls", &[]);
        assert!(
            matches!(result, Err(PluginError::PermissionDenied(_))),
            "白名单外命令应拒绝"
        );
    }

    #[test]
    fn test_exec_command_empty_whitelist_denies_all() {
        // bare system_command = 空白名单 → 任何命令都拒（fail-closed）。
        let (ctx, _temp) = create_test_context(vec![Capability::SystemCommand {
            allowed_commands: vec![],
        }]);
        let result = ctx.exec_command("echo", &[]);
        assert!(
            matches!(result, Err(PluginError::PermissionDenied(_))),
            "空白名单应 fail-closed 拒绝所有命令"
        );
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
        // userinfo 混淆：真实 host 在最后一个 '@' 之后（防 SSRF 用 userinfo 伪装白名单域）
        assert_eq!(
            extract_host("https://u:p@127.0.0.1/x").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            extract_host("https://allowed.com@evil.com/").as_deref(),
            Some("evil.com")
        );
        // 方括号 IPv6 + 端口（此前 split(':') 截到 '[' 致 is_internal_ip 层失效）
        assert_eq!(extract_host("https://[::1]:443/").as_deref(), Some("::1"));
        assert_eq!(
            extract_host("https://[2002:c0a8:0101::]/").as_deref(),
            Some("2002:c0a8:0101::")
        );
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
        assert!(is_internal_ip("::ffff:127.0.0.1")); // IPv4-mapped loopback（防旁路）
        assert!(is_internal_ip("::ffff:169.254.169.254")); // mapped 云元数据
        assert!(is_internal_ip("::192.168.1.1")); // IPv4-compatible 封装私网（RFC 4291 弃用，防旁路）
        assert!(is_internal_ip("fc00::1")); // IPv6 站点本地（unique local，≈私网）
        assert!(is_internal_ip("fe80::1")); // IPv6 链路本地
        assert!(is_internal_ip("2002:c0a8:0101::")); // 6to4 封装 192.168.1.1（防旁路）
        assert!(is_internal_ip("64:ff9b::7f00:1")); // NAT64 封装 127.0.0.1（防旁路）
        assert!(!is_internal_ip("8.8.8.8")); // 公网
        assert!(!is_internal_ip("2001:4860:4860::8888")); // 公网 IPv6
        assert!(!is_internal_ip("2002:0808:0808::")); // 6to4 封装 8.8.8.8（公网→放行，非误拦）
        assert!(!is_internal_ip("example.com")); // 域名（非 IP 字面量）
    }

    #[test]
    fn test_check_network_domain_suffix_confusion() {
        // 后缀混淆攻击防御：evilallowed.com 不应被 ["allowed.com"] 匹配
        // （须精确匹配或 ".allowed.com" 子域后缀）。
        let allowed = vec!["allowed.com".to_string()];
        assert!(!check_network_domain("http://evilallowed.com/", &allowed));
        assert!(!check_network_domain("http://notallowed.com/", &allowed));
        assert!(!check_network_domain("http://allowed.com.evil.com/", &allowed));
        assert!(check_network_domain("http://allowed.com/", &allowed));
        assert!(check_network_domain("http://sub.allowed.com/", &allowed));
        assert!(check_network_domain("http://anything.com/", &["*".to_string()]));
        assert!(!check_network_domain("not-a-url", &allowed));
    }

    #[test]
    fn test_filter_public_addrs_dns_rebinding() {
        use std::net::SocketAddr;
        let sa = |s: &str| s.parse::<SocketAddr>().unwrap();
        // 混合：127.0.0.1 滤掉，保留 8.8.8.8（IP pinning——解析到内网被滤）
        let public = filter_public_addrs(vec![sa("127.0.0.1:80"), sa("8.8.8.8:80")]).unwrap();
        assert_eq!(public.len(), 1);
        assert_eq!(
            public[0].ip(),
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(8, 8, 8, 8))
        );
        // 全内网 → Err（DNS rebinding 拒）
        assert!(filter_public_addrs(vec![sa("127.0.0.1:80"), sa("10.0.0.1:80")]).is_err());
        // 全公网 → 原样返回
        let all = filter_public_addrs(vec![sa("8.8.8.8:80"), sa("1.1.1.1:80")]).unwrap();
        assert_eq!(all.len(), 2);
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

    #[test]
    fn test_exec_command_rejects_nul_byte_in_arg() {
        // NUL 字节在参数中会导致 OS 截断，必须在执行前拒绝。
        let (ctx, _tmp) = create_test_context(vec![
            Capability::SystemCommand { allowed_commands: vec!["echo".to_string()] },
        ]);
        let args = vec!["hello\0world".to_string()];
        let result = ctx.exec_command("echo", &args);
        assert!(result.is_err(), "含 NUL 字节的参数应被拒绝");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("NUL"), "错误信息应提及 NUL: {msg}");
    }

    #[test]
    fn test_read_file_rejects_oversized_file() {
        // 文件超过 MAX_READ_FILE_BYTES(8MiB) → PermissionDenied（防 OOM）。
        // 用稀疏文件（seek + write 1 byte）模拟大文件，避免实际写 8MB 磁盘数据。
        let (ctx, tmp) = create_test_context(vec![Capability::ReadProject]);
        let large_path = tmp.path().join("large.bin");
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::File::create(&large_path).unwrap();
            // 8 MiB + 1 字节 → 超限
            f.seek(SeekFrom::Start(8 * 1024 * 1024)).unwrap();
            f.write_all(b"x").unwrap();
        }
        let result = ctx.read_file("large.bin");
        assert!(
            matches!(result, Err(PluginError::PermissionDenied(_))),
            "超大文件应被 PermissionDenied 拒绝: {:?}",
            result
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("过大") || msg.contains("上限"),
            "错误信息应提及文件过大: {msg}"
        );
    }
}
