//! R26 调用级运行遥测（v5 五轮规划 P0，403 号）。
//!
//! TS 真源：`packages/core/src/utils/run-log.ts`。每次 LLM 调用（链内一轮）
//! 追加元数据到进程级环形缓冲——**只记元数据，绝不记 prompt/正文/错误原文**
//! （v5 隐私红线；错误原文走 inkos.log 的 R25 既有通道）。
//! 共享向量 `packages/core/src/__tests__/golden/run-log-vectors.json`
//! 差分：`tests/golden_run_log_diff.rs`。

use std::sync::Mutex;
use std::sync::OnceLock;

pub const DEFAULT_RUN_LOG_CAPACITY: usize = 200;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogEntry {
    /// ISO 8601 UTC 时间戳（记录时刻）。
    pub ts: String,
    /// agent 名 / 调用标签。
    pub agent: String,
    pub model: String,
    /// 调用耗时（毫秒，整数）。
    pub duration_ms: i64,
    pub ok: bool,
    /// 链内序号（0 = primary）。
    pub attempt_index: i64,
    /// 轮次（1 起）。
    pub round: i64,
    /// R25 联动：非 primary 首轮命中即接管切换。
    pub took_over: bool,
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogSnapshot {
    /// 缓冲内现存记录（旧 → 新）。
    pub entries: Vec<RunLogEntry>,
    /// 累计追加数（含被逐出的旧记录）。
    pub total_appended: i64,
}

pub struct RunLogBuffer {
    items: Vec<RunLogEntry>,
    total_appended: i64,
    capacity: usize,
}

impl RunLogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { items: Vec::new(), total_appended: 0, capacity: capacity.max(1) }
    }

    pub fn append(&mut self, entry: RunLogEntry) {
        self.total_appended += 1;
        self.items.push(entry);
        if self.items.len() > self.capacity {
            let excess = self.items.len() - self.capacity;
            self.items.drain(..excess);
        }
    }

    pub fn snapshot(&self) -> RunLogSnapshot {
        RunLogSnapshot { entries: self.items.clone(), total_appended: self.total_appended }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogProjection {
    /// 累计调用数（含被逐出）。
    pub total: i64,
    /// 缓冲内现存条数。
    pub kept: i64,
    /// 现存记录中接管成功的次数（R25 联动展示）。
    pub took_over_count: i64,
    /// 现存记录中失败次数。
    pub failure_count: i64,
    /// 最新在后（时间升序，环形缓冲自然序）。
    pub entries: Vec<RunLogEntry>,
}

/// 只读投影：聚合计数 + 可选截尾（limit = 返回最近 limit 条，仍按时间升序）。
/// limit 非正或缺省 → 全量。
pub fn project_run_log(snapshot: &RunLogSnapshot, limit: Option<i64>) -> RunLogProjection {
    let entries = match limit {
        Some(limit) if limit > 0 => {
            let skip = (snapshot.entries.len() as i64 - limit).max(0) as usize;
            snapshot.entries[skip.min(snapshot.entries.len())..].to_vec()
        }
        _ => snapshot.entries.clone(),
    };
    RunLogProjection {
        total: snapshot.total_appended,
        kept: snapshot.entries.len() as i64,
        took_over_count: snapshot.entries.iter().filter(|entry| entry.took_over).count() as i64,
        failure_count: snapshot.entries.iter().filter(|entry| !entry.ok).count() as i64,
        entries,
    }
}

/// 进程级单例（engine 进程内）。
pub fn global_run_log() -> &'static Mutex<RunLogBuffer> {
    static GLOBAL: OnceLock<Mutex<RunLogBuffer>> = OnceLock::new();
    GLOBAL.get_or_init(|| Mutex::new(RunLogBuffer::new(DEFAULT_RUN_LOG_CAPACITY)))
}

/// 便捷记录：构造条目（时间戳内部生成）并追加入全局缓冲。
pub fn record_run_log(entry: RunLogEntry) {
    if let Ok(mut buffer) = global_run_log().lock() {
        buffer.append(entry);
    }
}
