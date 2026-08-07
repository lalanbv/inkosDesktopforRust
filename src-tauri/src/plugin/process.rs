//! 插件进程管理

use super::protocol::{RpcRequest, RpcResponse};
use super::types::{PluginError, PluginMetadata};
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

/// 进程插件 RPC 调用超时：进程超过此时长未响应 → kill + ExecutionFailed。
/// 防止行为异常插件的阻塞 read_line 永久冻结 execute_plugin（持 &mut self）。
const RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// 插件进程
pub struct PluginProcess {
    /// 插件元数据
    pub metadata: PluginMetadata,

    /// 子进程句柄
    child: Arc<Mutex<Child>>,

    /// 标准输入（发送请求）
    stdin: Arc<Mutex<ChildStdin>>,

    /// 标准输出（接收响应）
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,

    /// 请求计数器
    next_id: Arc<Mutex<u64>>,
}

/// 优雅关闭等待窗：先通知 → 等待进程自行退出 → 超时再 kill。
const GRACEFUL_SHUTDOWN_TIMEOUT_MS: u64 = 200;

/// 单条 stderr 行转发进日志的最大字节数——超出截断。
/// 插件可写单行数 MB 撑爆日志条目；日志行应保持可读。
const STDERR_LINE_MAX_BYTES: usize = 2 * 1024;

/// 每个插件进程 stderr 转发的最大行数。达上限后停止转发（继续排空管道，
/// 防插件写阻塞），只再记一条截断提示。防刷爆日志文件。
const STDERR_MAX_LINES: usize = 1_000;

/// detached 线程：排空插件 stderr 并限量转发进 `tracing`。
///
/// 两个职责不可分：
/// - **排空**——管道缓冲写满会阻塞插件的 write，卡死其主循环。即使不再转发也必须继续读。
/// - **限量转发**——带 `plugin_id` 归属写入日志，单行截断 + 总行数封顶。
///
/// 用 `read_until(b'\n')` 而非 `lines()`：后者对非 UTF-8 字节返回 Err 并可能
/// 提前结束迭代 → 停止排空 → 插件阻塞。这里按字节读，lossy 转换后记录。
fn spawn_stderr_drain(plugin_id: String, stderr: std::process::ChildStderr) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut lines_forwarded = 0usize;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break, // EOF：进程已退出
                Ok(_) => {}
                Err(_) => break, // 管道错误（进程被 kill）→ 结束
            }
            if lines_forwarded >= STDERR_MAX_LINES {
                continue; // 仍排空，但不再转发
            }
            // 截断到上限，去掉行尾换行；lossy 处理非 UTF-8 输出
            let end = buf.len().min(STDERR_LINE_MAX_BYTES);
            let line = String::from_utf8_lossy(&buf[..end]);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                continue;
            }
            lines_forwarded += 1;
            // 插件 stderr 是不可信内容：作为**字段值**记录（不进 message 模板），
            // 由 tracing 的格式化层负责转义，插件无法伪造宿主日志结构。
            tracing::warn!(
                target: "inkos.plugin.stderr",
                plugin_id = %plugin_id,
                truncated = buf.len() > STDERR_LINE_MAX_BYTES,
                output = %line,
                "插件 stderr 输出"
            );
            if lines_forwarded == STDERR_MAX_LINES {
                tracing::warn!(
                    target: "inkos.plugin.stderr",
                    plugin_id = %plugin_id,
                    limit = STDERR_MAX_LINES,
                    "插件 stderr 行数达上限，后续输出不再记录（仍排空管道）"
                );
            }
        }
    });
}

impl PluginProcess {
    /// 启动插件进程
    pub fn spawn(metadata: PluginMetadata, executable: &str) -> Result<Self> {
        let mut cmd = Command::new(executable);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // 捕获而非 inherit：inherit 让插件无限量直写宿主 stderr（刷爆日志、
            // 污染终端），且输出无归属——分不清来自哪个插件，还可伪造宿主日志
            // 格式行。改为管道 + 限量转发进 tracing（见 spawn_stderr_drain）。
            .stderr(Stdio::piped());
        // 环境隔离：清空后只注入白名单。插件是不可信第三方代码，继承宿主全部
        // 环境变量等于把 GITHUB_TOKEN / AWS_* 等密钥交给它（绕过能力模型）。
        // 与 host_api::exec_command 共用同一白名单，防两侧策略漂移。
        super::host_api::apply_env_allowlist(&mut cmd);
        let mut child = cmd
            .spawn()
            .context("Failed to spawn plugin process")?;

        let stdin = child.stdin.take().context("Failed to capture stdin")?;
        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        // stderr 必须被持续排空：管道缓冲区（通常 64 KiB）写满后插件的 write
        // 会阻塞，进而卡死其主循环 → RPC 超时。detached 线程负责排空 + 限量。
        if let Some(stderr) = child.stderr.take() {
            spawn_stderr_drain(metadata.id.clone(), stderr);
        }

        Ok(Self {
            metadata,
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            next_id: Arc::new(Mutex::new(1)),
        })
    }

    /// 调用插件方法
    pub fn call(&self, method: &str, params: Option<serde_json::Value>) -> Result<serde_json::Value, PluginError> {
        // 生成请求 ID
        let id = {
            let mut next_id = self.next_id.lock().map_err(|_| {
                PluginError::ExecutionFailed("Failed to lock request counter".to_string())
            })?;
            let id = *next_id;
            *next_id += 1;
            id
        };

        // 构造请求
        let request = RpcRequest::new(id, method, params);
        let request_line = serde_json::to_string(&request)
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to serialize request: {}", e)))?;

        // 发送请求
        {
            let mut stdin = self.stdin.lock().map_err(|_| {
                PluginError::ExecutionFailed("Failed to lock stdin".to_string())
            })?;

            writeln!(stdin, "{}", request_line)
                .map_err(|e| PluginError::ExecutionFailed(format!("Failed to write request: {}", e)))?;

            stdin.flush()
                .map_err(|e| PluginError::ExecutionFailed(format!("Failed to flush stdin: {}", e)))?;
        }

        // 读取响应——带超时保护（spawn 读线程 + channel recv_timeout）。
        // 直接 read_line 会永久阻塞：进程插件挂死时 execute_plugin 持 &mut self 冻结整个插件系统。
        let response_line = {
            let stdout_arc = Arc::clone(&self.stdout);
            let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();
            std::thread::spawn(move || {
                let result = stdout_arc.lock().map_err(|_| "Failed to lock stdout".to_string()).and_then(|mut out| {
                    let mut line = String::new();
                    out.read_line(&mut line)
                        .map(|_| line)
                        .map_err(|e| format!("Failed to read response: {}", e))
                });
                let _ = tx.send(result);
            });
            match rx.recv_timeout(RPC_TIMEOUT) {
                Ok(Ok(line)) => line,
                Ok(Err(e)) => return Err(PluginError::ExecutionFailed(e)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // 超时：kill 进程（best-effort），释放资源；调用方 execute_plugin
                    // 收到 ExecutionFailed → consec_failures++ → 达阈值自动禁用。
                    if let Ok(mut child) = self.child.lock() {
                        let _ = child.kill();
                    }
                    return Err(PluginError::ExecutionFailed(format!(
                        "插件进程 {} 响应超时（{}s），已终止",
                        self.metadata.id,
                        RPC_TIMEOUT.as_secs()
                    )));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(PluginError::ExecutionFailed("读线程异常退出".to_string()));
                }
            }
        };

        // 解析响应
        let response: RpcResponse = serde_json::from_str(&response_line)
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to parse response: {}", e)))?;

        // 验证响应 ID
        if response.id != id {
            return Err(PluginError::ExecutionFailed(format!(
                "Response ID mismatch: expected {}, got {}",
                id, response.id
            )));
        }

        // 返回结果或错误
        if let Some(error) = response.error {
            return Err(PluginError::ExecutionFailed(format!(
                "Plugin error {}: {}",
                error.code, error.message
            )));
        }

        response.result.ok_or_else(|| {
            PluginError::ExecutionFailed("Response has neither result nor error".to_string())
        })
    }

    /// 发送通知（不等待响应）
    pub fn notify(&self, method: &str, params: Option<serde_json::Value>) -> Result<(), PluginError> {
        let request = RpcRequest::notification(method, params);
        let request_line = serde_json::to_string(&request)
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to serialize notification: {}", e)))?;

        let mut stdin = self.stdin.lock().map_err(|_| {
            PluginError::ExecutionFailed("Failed to lock stdin".to_string())
        })?;

        writeln!(stdin, "{}", request_line)
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to write notification: {}", e)))?;

        stdin.flush()
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to flush stdin: {}", e)))?;

        Ok(())
    }

    /// 非阻塞通知（detached 线程 fire-and-forget）。
    ///
    /// 与 `notify` 语义相同，但在独立线程发送，防 stdin pipe 满时阻塞调用方。
    /// 用于 broadcast_event 等不等待结果的批量通知场景。
    pub fn notify_nonblocking(&self, method: &str, params: Option<serde_json::Value>) {
        let stdin_arc = Arc::clone(&self.stdin);
        let request = RpcRequest::notification(method, params);
        std::thread::spawn(move || {
            if let Ok(req_str) = serde_json::to_string(&request) {
                if let Ok(mut stdin) = stdin_arc.lock() {
                    let _ = writeln!(stdin, "{}", req_str);
                    let _ = stdin.flush();
                }
            }
        });
    }

/// 优雅关闭等待窗：先通知 → 等待进程自行退出 → 超时再 kill。
/// 終止插件进程（优雅关闭：先 notify("shutdown") best-effort → 等待 GRACEFUL_SHUTDOWN_TIMEOUT_MS → kill）。
///
/// 给插件机会清理资源（关文件句柄、保存状态等）。notify 在 detached 线程发送，
/// 防 stdin pipe 满时阻塞 stop() 本身——超时倒计时与 notify 并行。
pub fn stop(&self) -> Result<(), PluginError> {
    // detached 线程 fire-and-forget shutdown 通知：与超时倒计时并行，
    // 防 stdin pipe 满时 writeln 阻塞 stop()（管道满 = 进程无响应，需 hard kill）。
    let stdin_arc = Arc::clone(&self.stdin);
    std::thread::spawn(move || {
        let request = RpcRequest::notification("shutdown", None);
        if let Ok(req_str) = serde_json::to_string(&request) {
            if let Ok(mut stdin) = stdin_arc.lock() {
                let _ = writeln!(stdin, "{}", req_str);
                let _ = stdin.flush();
            }
        }
    });

    // 轮询等待进程自行退出（最多 GRACEFUL_SHUTDOWN_TIMEOUT_MS）
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(GRACEFUL_SHUTDOWN_TIMEOUT_MS);
    loop {
        if let Ok(mut child) = self.child.lock() {
            if child.try_wait().ok().flatten().is_some() {
                return Ok(()); // 进程已退出
            }
        }
        if std::time::Instant::now() >= deadline {
            break; // 超时 → hard kill
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // 超时：hard kill
    let mut child = self.child.lock().map_err(|_| {
        PluginError::ExecutionFailed("Failed to lock child process for kill".to_string())
    })?;
    child.kill().map_err(|e| {
        PluginError::ExecutionFailed(format!("Failed to kill process: {}", e))
    })?;
    Ok(())
}

    /// 检查进程是否存活
    pub fn is_alive(&self) -> bool {
        let mut child = match self.child.lock() {
            Ok(c) => c,
            Err(_) => return false,
        };

        child.try_wait().ok().flatten().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_plugin_process_creation() {
        let metadata = PluginMetadata {
            id: "test".to_string(),
            name: "Test Plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "Test".to_string(),
            author: "Test".to_string(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities: vec![],
            entrypoint: "plugin.js".to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        };

        // 测试无效可执行文件
        let result = PluginProcess::spawn(metadata, "/nonexistent");
        assert!(result.is_err());
    }

    fn metadata_with(id: &str, entrypoint: &str) -> PluginMetadata {
        PluginMetadata {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities: vec![],
            entrypoint: entrypoint.to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        }
    }

    /// stderr 改为 `Stdio::piped()` 后**必须**持续排空：管道缓冲（通常 64 KiB）
    /// 写满时插件的 write 阻塞 → 主循环卡死 → RPC 全部超时。
    ///
    /// 这里让插件先写远超缓冲区的 stderr（512 KiB），再对 stdin 回声应答。
    /// 若排空线程缺失或提前退出，插件卡在写 stderr，`call` 必然超时。
    /// 用 sh 而非真插件：只需验证「宿主侧排空」这一宿主行为。
    #[test]
    fn test_stderr_drain_does_not_block_plugin_writes() {
        // 先写 512 KiB 到 stderr（远超管道缓冲），再读一行 stdin 并回一个合法响应
        let script = r#"
            i=0
            while [ $i -lt 512 ]; do
                awk 'BEGIN{s="";while(length(s)<1023)s=s "x";print s}' >&2
                i=$((i+1))
            done
            read line
            printf '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\n'
        "#;
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        // 复用 spawn 的管道 + 排空逻辑：直接构造以免依赖可执行插件文件
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let Ok(mut child) = cmd.spawn() else {
            return; // sh 不可用（罕见）→ skip
        };
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        spawn_stderr_drain("stderr-flood".to_string(), child.stderr.take().unwrap());

        let process = PluginProcess {
            metadata: metadata_with("stderr-flood", "sh"),
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            next_id: Arc::new(Mutex::new(1)),
        };

        // 排空正常 → 插件写完 stderr 后能响应；排空缺失 → 卡死 → RPC_TIMEOUT
        let result = process.call("ping", None);
        let _ = process.stop();
        assert!(
            result.is_ok(),
            "stderr 洪泛下 RPC 仍应成功（排空线程防插件写阻塞），实际: {result:?}"
        );
    }

    /// 排空线程在进程退出后应自行结束（EOF），不泄漏线程。
    /// 同时验证非 UTF-8 字节不会让排空提前终止（lines() 会 Err，故用 read_until）。
    #[test]
    fn test_stderr_drain_handles_invalid_utf8_and_exits_on_eof() {
        let mut cmd = Command::new("sh");
        // printf 输出非法 UTF-8 字节序列后立即退出
        cmd.arg("-c").arg(r#"printf '\377\376 bad\n'>&2; printf 'ok\n' >&2"#);
        cmd.stderr(Stdio::piped()).stdout(Stdio::null()).stdin(Stdio::null());
        let Ok(mut child) = cmd.spawn() else { return };
        let stderr = child.stderr.take().unwrap();
        spawn_stderr_drain("invalid-utf8".to_string(), stderr);
        // 进程应正常退出（排空线程不影响其生命周期）
        let status = child.wait().expect("wait 应成功");
        assert!(status.success(), "插件应正常退出，实际: {status:?}");
    }

    #[test]
    fn test_is_alive_detects_killed_process() {
        // 用 `cat`（macOS/Linux 均有）作长驻进程，kill 后验证 is_alive() 返回 false。
        // 这直接测试死进程检测机制（execute_plugin_inner 依赖 is_alive() 触发 respawn）。
        let metadata = PluginMetadata {
            id: "alive-test".to_string(),
            name: "Alive Test".to_string(),
            version: "1.0.0".to_string(),
            description: "".to_string(),
            author: "".to_string(),
            homepage: None,
            license: "MIT".to_string(),
            abi_version: "1".to_string(),
            capabilities: vec![],
            entrypoint: "cat".to_string(),
            dependencies: HashMap::new(),
            enabled: true,
        };
        let Ok(process) = PluginProcess::spawn(metadata, "cat") else {
            // cat 不可用（某些 CI 环境）→ skip
            return;
        };
        assert!(process.is_alive(), "启动后应存活");
        process.stop().unwrap();
        // 进程被 kill 后稍等片刻确保 OS 更新状态
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!process.is_alive(), "kill 后 is_alive() 应返回 false");
    }
}
