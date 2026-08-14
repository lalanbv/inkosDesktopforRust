//! studio 任务快照存储（`.inkos/tasks/{sessionId}.json`）。
//!
//! 移植自 `packages/studio/src/api/task-store.ts`（112 行）。write-next /
//! create 等异步任务的磁盘快照：每会话一个 JSON 文件，**同路径写入串行化**
//! （TS 的 per-path promise 队列 → Rust 的 per-path tokio 互斥锁等价还原）。
//!
//! ## 移植纪律
//! - 文件名 = `encodeURIComponent(sessionId) + ".json"`——JS 组件编码的
//!   保留集（`A-Za-z0-9-_.!~*'()`）逐字对齐
//! - 解析是**字段级宽松校验**：requestedIntent 只验 string（TS 同语义，
//!   不验枚举值）；version 必须为 1；logs 元素必须全 string
//! - 写入后带尾随 `\n`；目录递归创建；读/删前等待在途写队列

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// 任务执行状态。对齐 TS `StudioTaskExecutionStatus`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StudioTaskExecutionStatus {
    Running,
    Processing,
    Completed,
    Error,
}

/// 阶段状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StudioTaskStageStatus {
    Pending,
    Active,
    Completed,
}

/// 任务阶段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskStage {
    pub label: String,
    pub status: StudioTaskStageStatus,
}

/// 任务执行体。对齐 TS `StudioTaskExecution`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskExecution {
    pub id: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub label: String,
    pub status: StudioTaskExecutionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stages: Option<Vec<StudioTaskStage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<Vec<String>>,
    pub started_at: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<f64>,
}

/// 任务快照。对齐 TS `StudioTaskSnapshot`（version 固定 1）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioTaskSnapshot {
    pub version: u32,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_request_id: Option<String>,
    /// TS 解析只验 string（不验枚举值）——保持宽松语义。
    pub requested_intent: String,
    pub execution: StudioTaskExecution,
    pub updated_at: f64,
}

/// 任务目录（项目根相对）。
const TASKS_DIR: &str = ".inkos/tasks";

/// per-path 写入串行锁（TS writeQueues 的等价物）。
fn write_locks() -> &'static Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn path_lock(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let lock = {
        let mut locks = write_locks().lock().unwrap();
        locks.entry(path.to_path_buf()).or_default().clone()
    };
    // 持锁期间移除空闲条目会破坏排队语义（新写入者拿新锁绕过队列）；
    // TS 仅在队尾仍是自己时清理——等价做法：保留条目（少量路径常驻，
    // studio 会话数有限，无泄漏风险）。
    lock
}

/// JS `encodeURIComponent` 的保留集逐字对齐（A-Za-z0-9 - _ . ! ~ * ' ( )）。
pub fn js_encode_uri_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')');
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn task_file_name(session_id: &str) -> String {
    format!("{}.json", js_encode_uri_component(session_id))
}

/// 快照落盘路径。
pub fn studio_task_snapshot_path(project_root: &Path, session_id: &str) -> PathBuf {
    project_root.join(TASKS_DIR).join(task_file_name(session_id))
}

/// 字段级校验解析（坏载荷 → None）。
pub fn parse_studio_task_snapshot(value: &serde_json::Value) -> Option<StudioTaskSnapshot> {
    // 结构级先走 serde（含 version/枚举校验），再做 TS 的显式字段检查等价物。
    let snapshot: StudioTaskSnapshot = serde_json::from_value(value.clone()).ok()?;
    if snapshot.version != 1 {
        return None;
    }
    // TS requestedIntent 只验 string；serde 已保证。
    if snapshot.session_id.is_empty() && value.get("sessionId").and_then(|v| v.as_str()) != Some("") {
        return None;
    }
    Some(snapshot)
}

/// 保存快照（同路径串行；尾随换行）。
pub async fn save_studio_task_snapshot(
    project_root: &Path,
    snapshot: &StudioTaskSnapshot,
) -> std::io::Result<()> {
    let path = studio_task_snapshot_path(project_root, &snapshot.session_id);
    let lock = path_lock(&path).await;
    let _guard = lock.lock().await;

    tokio::fs::create_dir_all(project_root.join(TASKS_DIR)).await?;
    let mut serialized = serde_json::to_string_pretty(snapshot)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    serialized.push('\n');
    tokio::fs::write(&path, serialized).await
}

/// 读取快照（等待在途写；缺失/坏载荷 → None）。
pub async fn load_studio_task_snapshot(
    project_root: &Path,
    session_id: &str,
) -> Option<StudioTaskSnapshot> {
    let path = studio_task_snapshot_path(project_root, session_id);
    let lock = path_lock(&path).await;
    let _guard = lock.lock().await;

    let raw = tokio::fs::read_to_string(&path).await.ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    parse_studio_task_snapshot(&value)
}

/// 删除快照（等待在途写；缺失静默）。
pub async fn delete_studio_task_snapshot(project_root: &Path, session_id: &str) {
    let path = studio_task_snapshot_path(project_root, session_id);
    let lock = path_lock(&path).await;
    let _guard = lock.lock().await;

    let _ = tokio::fs::remove_file(&path).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(session_id: &str) -> StudioTaskSnapshot {
        StudioTaskSnapshot {
            version: 1,
            session_id: session_id.to_string(),
            source_request_id: Some("req-1".to_string()),
            requested_intent: "write_next".to_string(),
            execution: StudioTaskExecution {
                id: "task-1".to_string(),
                tool: "pipeline".to_string(),
                agent: Some("writer".to_string()),
                label: "撰写第 3 章".to_string(),
                status: StudioTaskExecutionStatus::Processing,
                args: None,
                result: None,
                details: Some(serde_json::json!({ "chapter": 3 })),
                error: None,
                stages: Some(vec![
                    StudioTaskStage { label: "规划".into(), status: StudioTaskStageStatus::Completed },
                    StudioTaskStage { label: "撰写".into(), status: StudioTaskStageStatus::Active },
                    StudioTaskStage { label: "落盘".into(), status: StudioTaskStageStatus::Pending },
                ]),
                logs: Some(vec!["stage: 规划完成".to_string()]),
                started_at: 1_786_752_000.0,
                completed_at: None,
            },
            updated_at: 1_786_752_001.0,
        }
    }

    #[tokio::test]
    async fn snapshot_roundtrip_with_pretty_json_and_newline() {
        let dir = tempfile::tempdir().unwrap();
        save_studio_task_snapshot(dir.path(), &sample("sess-1")).await.unwrap();
        let path = studio_task_snapshot_path(dir.path(), "sess-1");
        let raw = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(raw.ends_with("}\n"));
        assert!(raw.contains("\"requestedIntent\": \"write_next\""));
        let loaded = load_studio_task_snapshot(dir.path(), "sess-1").await.unwrap();
        assert_eq!(loaded, sample("sess-1"));
    }

    #[tokio::test]
    async fn load_missing_returns_none_delete_silent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_studio_task_snapshot(dir.path(), "nope").await.is_none());
        delete_studio_task_snapshot(dir.path(), "nope").await;
    }

    #[tokio::test]
    async fn concurrent_saves_serialize_per_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let mut handles = Vec::new();
        for i in 0..16 {
            let root = root.clone();
            handles.push(tokio::spawn(async move {
                let mut snapshot = sample("busy");
                snapshot.updated_at = 1000.0 + i as f64;
                save_studio_task_snapshot(&root, &snapshot).await.unwrap();
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        // 最终态是某一次完整写入（无交错撕裂）。
        let loaded = load_studio_task_snapshot(&root, "busy").await.unwrap();
        assert!((1000.0..=1015.0).contains(&loaded.updated_at));
    }

    #[test]
    fn session_id_uri_component_encoding() {
        assert_eq!(js_encode_uri_component("abc-XYZ_1"), "abc-XYZ_1");
        assert_eq!(js_encode_uri_component("a.b!~*'()"), "a.b!~*'()");
        // 中文与特殊字符全转义（UTF-8 百分号大写十六进制）。
        assert_eq!(js_encode_uri_component("会话"), "%E4%BC%9A%E8%AF%9D");
        assert_eq!(js_encode_uri_component("a/b c"), "a%2Fb%20c");
        assert_eq!(task_file_name("会话/1"), "%E4%BC%9A%E8%AF%9D%2F1.json");
    }

    #[test]
    fn parse_rejects_bad_payloads() {
        let good = serde_json::to_value(sample("s")).unwrap();
        assert!(parse_studio_task_snapshot(&good).is_some());

        // version != 1。
        let mut bad = good.clone();
        bad["version"] = serde_json::json!(2);
        assert!(parse_studio_task_snapshot(&bad).is_none());

        // 缺 execution。
        let mut bad = good.clone();
        bad.as_object_mut().unwrap().remove("execution");
        assert!(parse_studio_task_snapshot(&bad).is_none());

        // status 非法。
        let mut bad = good.clone();
        bad["execution"]["status"] = serde_json::json!("weird");
        assert!(parse_studio_task_snapshot(&bad).is_none());

        // startedAt 非数值。
        let mut bad = good.clone();
        bad["execution"]["startedAt"] = serde_json::json!("not-a-number");
        assert!(parse_studio_task_snapshot(&bad).is_none());

        // logs 混入非字符串。
        let mut bad = good;
        bad["execution"]["logs"] = serde_json::json!(["ok", 42]);
        assert!(parse_studio_task_snapshot(&bad).is_none());

        // 非 object。
        assert!(parse_studio_task_snapshot(&serde_json::json!([])).is_none());
    }
}
