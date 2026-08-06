//! 插件进程管理

use super::protocol::{RpcRequest, RpcResponse};
use super::types::{PluginError, PluginMetadata};
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

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

        // 读取响应
        let response_line = {
            let mut stdout = self.stdout.lock().map_err(|_| {
                PluginError::ExecutionFailed("Failed to lock stdout".to_string())
            })?;

            let mut line = String::new();
            stdout.read_line(&mut line)
                .map_err(|e| PluginError::ExecutionFailed(format!("Failed to read response: {}", e)))?;
            line
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

    /// 终止插件进程
    pub fn stop(&self) -> Result<(), PluginError> {
        let mut child = self.child.lock().map_err(|_| {
            PluginError::ExecutionFailed("Failed to lock child process".to_string())
        })?;

        child.kill()
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to kill process: {}", e)))?;

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
}
