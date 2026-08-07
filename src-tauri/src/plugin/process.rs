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

impl PluginProcess {
    /// 启动插件进程
    pub fn spawn(metadata: PluginMetadata, executable: &str) -> Result<Self> {
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // 错误输出到主进程 stderr
            .spawn()
            .context("Failed to spawn plugin process")?;

        let stdin = child.stdin.take().context("Failed to capture stdin")?;
        let stdout = child.stdout.take().context("Failed to capture stdout")?;

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

/// 优雅关闭等待窗：先通知 → 等待进程自行退出 → 超时再 kill。
/// 終止插件进程（优雅关闭：先 notify("shutdown") best-effort → 等待 GRACEFUL_SHUTDOWN_TIMEOUT_MS → kill）。
///
/// 给插件机会清理资源（关文件句柄、保存状态等）。notify 失败（进程已死/pipe 断）不阻断关闭。
pub fn stop(&self) -> Result<(), PluginError> {
    // best-effort 优雅通知（已死进程的 writeln 会直接 Err，忽略即可）
    let _ = self.notify("shutdown", None);

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
