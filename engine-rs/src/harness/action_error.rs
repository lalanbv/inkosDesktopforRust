//! 能力动作错误（884 号 M1b）：TS `Object.assign(new Error(msg), {code, recovery, …})`
//! 形状的 Rust 对应。`failure_facts` = `action-observation.ts#actionFailureFacts`
//! 移植——action-failed 事件载荷的事实提取（code/recovery 顶层字段）。

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ActionError {
    pub message: String,
    pub code: Option<String>,
    /// recovery 载荷装箱（保持 Err 变体低于 clippy result_large_err 阈值——
    /// 门禁零容忍，无 allow 通道）。
    pub recovery: Option<Box<Value>>,
    /// ARTIFACT_DOMAIN_ACTION_REQUIRED / ARTIFACT_STRUCTURE_CHANGED 携带。
    pub path: Option<String>,
    pub capability_id: Option<String>,
}

impl ActionError {
    pub fn plain(message: impl Into<String>) -> Self {
        Self { message: message.into(), code: None, recovery: None, path: None, capability_id: None }
    }

    pub fn coded(message: impl Into<String>, code: &str) -> Self {
        Self {
            message: message.into(),
            code: Some(code.to_string()),
            recovery: None,
            path: None,
            capability_id: None,
        }
    }

    pub fn with_recovery(mut self, recovery: Value) -> Self {
        self.recovery = Some(Box::new(recovery));
        self
    }

    /// TS `actionFailureFacts(error)`：action-failed 事件载荷的 error 之外字段
    ///（message 本身由调用方以 `error` 键写入）。白名单 = code/recovery——TS 提取
    /// 的 resultTool/stopReason/attempts 属 LLM 轨道字段，adopt 路径不产生；
    /// path/capabilityId 挂在错误对象上但不入 facts（TS 同）。
    pub fn failure_facts(&self) -> Value {
        let mut payload = serde_json::Map::new();
        if let Some(code) = &self.code {
            payload.insert("code".into(), Value::String(code.clone()));
        }
        if let Some(recovery) = &self.recovery {
            payload.insert("recovery".into(), (**recovery).clone());
        }
        Value::Object(payload)
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ActionError {}
