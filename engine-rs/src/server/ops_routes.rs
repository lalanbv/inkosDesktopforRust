//! 运维面端点（72 号）：daemon 三条 + logs + doctor + radar 两条。
//!
//! 契约来源 `packages/studio/src/api/server.ts` L4463-L4536（daemon/logs）、
//! L6434-L6456（radar）、L6459-L6500（doctor）+ `packages/core/src/pipeline/
//! scheduler.ts`（cronToMs 间隔近似 + 写循环策略）。
//!
//! Scheduler 为精简生命周期（偏差备案见 72 号记录）：
//! - cronToMs 逐字（`*/N` 分/时 → 间隔，否则每日）；tick 重入跳过
//! - 写循环：日上限（YYYY-MM-DD 计数）+ active/outlining 书前 N 本并发 +
//!   每书 chaptersPerCycle 章（冷却 + 重试温度步进 + 连续失败暂停）
//! - radar tick：run_radar → 落盘（与 POST /radar/scan 同链）
//! - 暂缓：detection 自动改写环 / webhook 通知 / 失败维度聚类

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::server::books_routes::BooksRuntime;

fn flat_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

// ── daemon：Scheduler 精简生命周期 ───────────────────────────────

struct SchedulerHandle {
    running: Arc<std::sync::atomic::AtomicBool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

fn scheduler_slot() -> &'static Mutex<Option<SchedulerHandle>> {
    static SLOT: OnceLock<Mutex<Option<SchedulerHandle>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// daemon 配置段（project.ts daemon schema 默认值）。
#[derive(Debug, Clone)]
struct DaemonConfig {
    radar_cron: String,
    write_cron: String,
    max_concurrent_books: usize,
    chapters_per_cycle: usize,
    retry_delay_ms: u64,
    cooldown_after_chapter_ms: u64,
    max_chapters_per_day: usize,
}

impl DaemonConfig {
    fn from_raw(raw: Option<Value>) -> Self {
        let daemon = raw.as_ref().and_then(|config| config.get("daemon"));
        let get_str = |name: &str, default: &str| -> String {
            daemon
                .and_then(|d| d.get("schedule"))
                .and_then(|s| s.get(name))
                .or_else(|| daemon.and_then(|d| d.get(name)))
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| default.to_string())
        };
        let get_num = |name: &str, default: u64| -> u64 {
            daemon
                .and_then(|d| d.get(name))
                .and_then(Value::as_u64)
                .unwrap_or(default)
        };
        Self {
            radar_cron: get_str("radarCron", "0 */6 * * *"),
            write_cron: get_str("writeCron", "*/15 * * * *"),
            max_concurrent_books: (get_num("maxConcurrentBooks", 3) as usize).max(1),
            chapters_per_cycle: (get_num("chaptersPerCycle", 1) as usize).clamp(1, 20),
            retry_delay_ms: get_num("retryDelayMs", 30_000),
            cooldown_after_chapter_ms: get_num("cooldownAfterChapterMs", 10_000),
            max_chapters_per_day: (get_num("maxChaptersPerDay", 50) as usize).max(1),
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            radar_cron: "0 */6 * * *".to_string(),
            write_cron: "*/15 * * * *".to_string(),
            max_concurrent_books: 3,
            chapters_per_cycle: 1,
            retry_delay_ms: 30_000,
            cooldown_after_chapter_ms: 10_000,
            max_chapters_per_day: 50,
        }
    }
}

/// `cronToMs`（scheduler.ts 逐字）：`*/N` 分钟 / `0 */N` 小时 / 其它每日。
pub fn cron_to_ms(cron: &str) -> u64 {
    let parts: Vec<&str> = cron.split(' ').collect();
    if parts.len() < 5 {
        return 24 * 60 * 60 * 1000;
    }
    let minute = parts[0];
    let hour = parts[1];
    if let Some(interval) = minute.strip_prefix("*/") {
        return interval.parse::<u64>().unwrap_or(24 * 60 * 60) * 60 * 1000;
    }
    if let Some(interval) = hour.strip_prefix("*/") {
        return interval.parse::<u64>().unwrap_or(24) * 60 * 60 * 1000;
    }
    24 * 60 * 60 * 1000
}

fn today_key() -> String {
    // UTC 日期（TS toISOString().slice(0, 10)）。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    crate::utils::utc_time::unix_to_utc_iso(secs, 0)[..10].to_string()
}

/// 写循环状态（Scheduler 内部 map 的进程级等价）。
#[derive(Default)]
struct WriteCycleState {
    consecutive_failures: HashMap<String, u32>,
    paused_books: HashSet<String>,
    daily_counts: HashMap<String, usize>,
}

fn write_cycle_state() -> &'static Mutex<WriteCycleState> {
    static STATE: OnceLock<Mutex<WriteCycleState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(WriteCycleState::default()))
}

async fn write_one_chapter(runtime: &BooksRuntime, book_id: &str, temperature: Option<f64>) -> Result<bool, String> {
    use crate::pipeline::write_next::{write_next_chapter, WriteNextConfig};
    let agents = crate::server::books_routes::build_write_next_agents(runtime);
    let ctx = crate::server::books_routes::build_write_next_ctx(runtime);
    let result = write_next_chapter(
        &runtime.state,
        &agents,
        &ctx,
        &WriteNextConfig::default(),
        book_id,
        None,
        temperature,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(result.status == "ready-for-review")
}

async fn run_write_cycle(runtime: &BooksRuntime, config: &DaemonConfig) {
    let (cap_reached, max_per_day) = {
        let state = write_cycle_state().lock().unwrap();
        (state.daily_counts.get(&today_key()).copied().unwrap_or(0) >= config.max_chapters_per_day, config.max_chapters_per_day)
    };
    if cap_reached {
        return;
    }
    let book_ids = runtime.state.list_books().await;
    let mut active_books = Vec::new();
    for book_id in book_ids {
        let paused = write_cycle_state()
            .lock()
            .unwrap()
            .paused_books
            .contains(&book_id);
        if paused {
            continue;
        }
        if let Ok(config) = runtime.state.load_book_config(&book_id).await {
            if matches!(config.status, crate::models::book::BookStatus::Active | crate::models::book::BookStatus::Outlining) {
                active_books.push(book_id);
            }
        }
    }
    let books: Vec<String> = active_books
        .into_iter()
        .take(config.max_concurrent_books)
        .collect();

    let mut book_tasks = Vec::new();
    for book_id in books {
        let runtime = runtime.clone();
        let config = config.clone();
        book_tasks.push(tokio::spawn(async move {
            process_book(&runtime, &config, &book_id).await;
        }));
    }
    for task in book_tasks {
        let _ = task.await;
    }
    let _ = max_per_day;
}

async fn process_book(runtime: &BooksRuntime, config: &DaemonConfig, book_id: &str) {
    const MAX_AUDIT_RETRIES: u32 = 2;
    const PAUSE_AFTER_CONSECUTIVE_FAILURES: u32 = 3;
    const RETRY_TEMPERATURE_STEP: f64 = 0.1;

    for i in 0..config.chapters_per_cycle {
        if !scheduler_running() {
            return;
        }
        {
            let state = write_cycle_state().lock().unwrap();
            if state.daily_counts.get(&today_key()).copied().unwrap_or(0) >= config.max_chapters_per_day {
                return;
            }
            if state.paused_books.contains(book_id) {
                return;
            }
        }
        if i > 0 && config.cooldown_after_chapter_ms > 0 {
            tokio::time::sleep(Duration::from_millis(config.cooldown_after_chapter_ms)).await;
        }

        let failures = write_cycle_state()
            .lock()
            .unwrap()
            .consecutive_failures
            .get(book_id)
            .copied()
            .unwrap_or(0);
        let temperature = if failures > 0 {
            Some((0.7 + failures as f64 * RETRY_TEMPERATURE_STEP).min(1.2))
        } else {
            None
        };

        let success = match write_one_chapter(runtime, book_id, temperature).await {
            Ok(success) => success,
            Err(error) => {
                runtime.hub.broadcast(
                    "daemon:error",
                    &json!({ "bookId": book_id, "error": error }),
                );
                false
            }
        };

        if success {
            write_cycle_state().lock().unwrap().consecutive_failures.remove(book_id);
            let mut state = write_cycle_state().lock().unwrap();
            let today = today_key();
            let count = state.daily_counts.get(&today).copied().unwrap_or(0) + 1;
            state.daily_counts.retain(|key, _| key == &today);
            state.daily_counts.insert(today, count);
            // onChapterComplete 广播的 chapter/status 简化为定值面（偏差备案）。
            runtime
                .hub
                .broadcast("daemon:chapter", &json!({ "bookId": book_id, "chapter": 0, "status": "ready-for-review" }));
            continue;
        }

        let failures = {
            let mut state = write_cycle_state().lock().unwrap();
            let failures = state.consecutive_failures.entry(book_id.to_string()).or_insert(0);
            *failures += 1;
            *failures
        };
        if failures >= PAUSE_AFTER_CONSECUTIVE_FAILURES {
            write_cycle_state().lock().unwrap().paused_books.insert(book_id.to_string());
        }
        if failures <= MAX_AUDIT_RETRIES && config.retry_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(config.retry_delay_ms)).await;
            let retry_temperature = Some((0.7 + failures as f64 * RETRY_TEMPERATURE_STEP).min(1.2));
            match write_one_chapter(runtime, book_id, retry_temperature).await {
                Ok(true) => {
                    write_cycle_state().lock().unwrap().consecutive_failures.remove(book_id);
                }
                _ => break,
            }
        } else {
            break;
        }
    }
}

fn scheduler_running() -> bool {
    scheduler_slot()
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|handle| handle.running.load(std::sync::atomic::Ordering::SeqCst))
}

async fn run_radar_scan_and_save(runtime: &BooksRuntime) -> Result<Value, String> {
    let result = crate::agents::radar::run_radar(&runtime.router).await?;
    let value = serde_json::to_value(&result).unwrap_or(Value::Null);
    save_radar_scan(runtime.state.project_root(), &value).await?;
    Ok(value)
}

// ── daemon 端点 ─────────────────────────────────────────────────

pub async fn get_daemon(State(_runtime): State<BooksRuntime>) -> impl IntoResponse {
    Json(json!({ "running": scheduler_running() }))
}

pub async fn post_daemon_start(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    if scheduler_running() {
        return flat_error(StatusCode::BAD_REQUEST, "Daemon already running");
    }
    let config = DaemonConfig::from_raw(
        crate::server::project_config_routes::load_raw_config(runtime.state.project_root()).await,
    );
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = running.clone();

    // 写循环（立即一轮 + 间隔 tick）。
    let write_runtime = runtime.clone();
    let write_config = config.clone();
    let write_flag = running.clone();
    let write_task = tokio::spawn(async move {
        let interval = Duration::from_millis(cron_to_ms(&write_config.write_cron));
        loop {
            if !write_flag.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            run_write_cycle(&write_runtime, &write_config).await;
            let mut waited = Duration::ZERO;
            while waited < interval {
                if !write_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let step = Duration::from_millis(500);
                tokio::time::sleep(step).await;
                waited += step;
            }
        }
    });

    // radar tick（间隔轮询；单飞重入跳过由 tick 间隔保证——与 TS inFlight 等价简化）。
    let radar_runtime = runtime.clone();
    let radar_config = config.clone();
    let radar_flag = running.clone();
    let radar_task = tokio::spawn(async move {
        let _ = radar_config;
        let interval = Duration::from_millis(cron_to_ms(&radar_config.radar_cron));
        loop {
            let mut elapsed = Duration::ZERO;
            while elapsed < interval {
                if !radar_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                let step = Duration::from_millis(500);
                tokio::time::sleep(step).await;
                elapsed += step;
            }
            if let Err(error) = run_radar_scan_and_save(&radar_runtime).await {
                radar_runtime
                    .hub
                    .broadcast("daemon:error", &json!({ "bookId": "radar", "error": error }));
            }
        }
    });

    scheduler_slot().lock().unwrap().replace(SchedulerHandle {
        running: flag,
        tasks: vec![write_task, radar_task],
    });
    runtime.hub.broadcast("daemon:started", &json!({}));
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "running": true })),
    )
        .into_response()
}

pub async fn post_daemon_stop(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let stopped = {
        let mut slot = scheduler_slot().lock().unwrap();
        match slot.as_ref() {
            Some(handle) if handle.running.load(std::sync::atomic::Ordering::SeqCst) => {
                handle
                    .running
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                let handle = slot.take().expect("as_ref 已验证");
                for task in handle.tasks {
                    task.abort();
                }
                true
            }
            _ => false,
        }
    };
    if !stopped {
        return flat_error(StatusCode::BAD_REQUEST, "Daemon not running");
    }
    runtime.hub.broadcast("daemon:stopped", &json!({}));
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "running": false })),
    )
        .into_response()
}

// ── logs ────────────────────────────────────────────────────────

pub async fn get_logs(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let path = runtime.state.project_root().join("inkos.log");
    let Ok(content) = tokio::fs::read_to_string(&path).await else {
        return Json(json!({ "entries": [] })).into_response();
    };
    let lines: Vec<&str> = content.trim().split('\n').collect();
    let tail: Vec<&str> = if lines.len() > 100 {
        lines[lines.len() - 100..].to_vec()
    } else {
        lines
    };
    let entries: Vec<Value> = tail
        .iter()
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(_) => json!({ "message": line }),
        })
        .collect();
    Json(json!({ "entries": entries })).into_response()
}

// ── radar 存储与端点 ────────────────────────────────────────────

fn radar_timestamp_for_filename(value: &str) -> String {
    // ISO 时间戳的 `[:.]` → `-`；无效回退当前。
    let timestamp = if value.is_empty() {
        crate::utils::utc_time::utc_now_iso()
    } else {
        value.to_string()
    };
    timestamp.replace([':', '.'], "-")
}

pub async fn save_radar_scan(root: &Path, result: &Value) -> Result<String, String> {
    let radar_dir = root.join("radar");
    tokio::fs::create_dir_all(&radar_dir).await.map_err(|e| e.to_string())?;
    let timestamp = result
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let file_name = format!("scan-{}.json", radar_timestamp_for_filename(timestamp));
    let file_path = radar_dir.join(&file_name);
    let payload = format!("{}\n", serde_json::to_string_pretty(result).unwrap_or_default());
    tokio::fs::write(&file_path, payload)
        .await
        .map_err(|e| e.to_string())?;
    Ok(file_path.to_string_lossy().to_string())
}

pub async fn load_radar_history(root: &Path) -> Result<Vec<Value>, String> {
    let radar_dir = root.join("radar");
    let mut files: Vec<String> = Vec::new();
    let mut entries = tokio::fs::read_dir(&radar_dir)
        .await
        .map_err(|e| e.to_string())?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(name) = entry.file_name().into_string() {
            files.push(name);
        }
    }
    let mut scans: Vec<Value> = Vec::new();
    for file in files {
        if !(file.starts_with("scan-") && file.ends_with(".json")) {
            continue;
        }
        let Ok(raw) = tokio::fs::read_to_string(radar_dir.join(&file)).await else {
            continue;
        };
        let Ok(result) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let timestamp = result
            .get("timestamp")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| {
                file.trim_start_matches("scan-")
                    .trim_end_matches(".json")
                    .to_string()
            });
        let market_summary = result
            .get("marketSummary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        scans.push(json!({
            "file": file,
            "timestamp": timestamp,
            "marketSummary": market_summary,
            "summaryPreview": market_summary.chars().take(100).collect::<String>(),
            "result": result,
        }));
    }
    scans.sort_by(|a, b| {
        let a = a["file"].as_str().unwrap_or_default();
        let b = b["file"].as_str().unwrap_or_default();
        b.cmp(a)
    });
    Ok(scans)
}

pub async fn post_radar_scan(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    runtime.hub.broadcast("radar:start", &json!({}));
    match run_radar_scan_and_save(&runtime).await {
        Ok(result) => {
            runtime.hub.broadcast("radar:complete", &json!({ "result": result }));
            (StatusCode::OK, Json(result)).into_response()
        }
        Err(message) => {
            runtime.hub.broadcast("radar:error", &json!({ "error": message }));
            flat_error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

pub async fn get_radar_history(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    match load_radar_history(runtime.state.project_root()).await {
        Ok(items) => (StatusCode::OK, Json(json!({ "items": items }))).into_response(),
        Err(message) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

// ── doctor ──────────────────────────────────────────────────────

pub async fn get_doctor(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let global_env = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".inkos").join(".env"))
        .unwrap_or_default();

    let mut checks = json!({
        "inkosJson": root.join("inkos.json").is_file(),
        "projectEnv": root.join(".env").is_file(),
        "globalEnv": global_env.is_file(),
        "booksDir": root.join("books").is_dir(),
        "llmConnected": false,
        "bookCount": 0,
    });
    let books = runtime.state.list_books().await;
    checks["bookCount"] = json!(books.len());
    // LLM 连通探测（99 号对齐 TS probeServiceCapabilities 主干）：models
    // GET → 不可达时 chat 深链（preferred stream → 空/失败回退非 stream），
    // 9 秒总预算（DOCTOR_LLM_PROBE_BUDGET_MS；慢/限流上游按未连接上报）。
    let probe = tokio::time::timeout(Duration::from_secs(9), async {
        let endpoint = runtime.router.resolve("radar");
        let base = endpoint.base_url.trim_end_matches('/').to_string();
        let url = format!("{base}/models");
        let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
        let mut request = client.get(&url);
        if !endpoint.api_key.is_empty() {
            request = request.bearer_auth(&endpoint.api_key);
        }
        if request.send().await.map(|response| response.status().is_success()).unwrap_or(false) {
            return true;
        }
        // 深链回退：候选 = 端点模型；计划 = llm.stream 优先流式 → 空/失败
        // 回退非流式（TS buildProbePlans preferred 分支）。
        let preferred_stream = crate::server::project_config_routes::load_raw_config(root)
            .await
            .and_then(|config| config.get("llm").cloned())
            .and_then(|llm| llm.get("stream").and_then(serde_json::Value::as_bool))
            .unwrap_or(false);
        let mut plans = vec![preferred_stream];
        if preferred_stream {
            plans.push(false);
        }
        for stream in plans {
            if crate::server::service_routes::minimal_chat_probe(
                &base,
                &endpoint.api_key,
                &endpoint.model,
                stream,
            )
            .await
            .is_ok()
            {
                return true;
            }
        }
        false
    })
    .await;
    if let Ok(true) = probe {
        checks["llmConnected"] = json!(true);
    }
    (StatusCode::OK, Json(checks)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_to_ms_matches_scheduler_semantics() {
        assert_eq!(cron_to_ms("*/15 * * * *"), 15 * 60 * 1000);
        assert_eq!(cron_to_ms("*/30 * * * *"), 30 * 60 * 1000);
        assert_eq!(cron_to_ms("0 */6 * * *"), 6 * 60 * 60 * 1000);
        assert_eq!(cron_to_ms("0 3 * * *"), 24 * 60 * 60 * 1000);
        assert_eq!(cron_to_ms("bad"), 24 * 60 * 60 * 1000);
    }

    #[test]
    fn today_key_is_utc_date_shape() {
        let key = today_key();
        assert_eq!(key.len(), 10);
        assert_eq!(key.as_bytes()[4], b'-');
        assert_eq!(key.as_bytes()[7], b'-');
    }

    #[test]
    fn daemon_config_defaults_and_overrides() {
        let defaults = DaemonConfig::from_raw(None);
        assert_eq!(defaults.radar_cron, "0 */6 * * *");
        assert_eq!(defaults.write_cron, "*/15 * * * *");
        assert_eq!(defaults.max_concurrent_books, 3);
        assert_eq!(defaults.chapters_per_cycle, 1);
        assert_eq!(defaults.max_chapters_per_day, 50);

        let raw: Value = serde_json::from_str(
            r#"{ "daemon": { "schedule": { "writeCron": "*/5 * * * *" }, "maxChaptersPerDay": 10 } }"#,
        )
        .unwrap();
        let config = DaemonConfig::from_raw(Some(raw));
        assert_eq!(config.write_cron, "*/5 * * * *");
        assert_eq!(config.radar_cron, "0 */6 * * *");
        assert_eq!(config.max_chapters_per_day, 10);
    }

    #[tokio::test]
    async fn radar_scan_roundtrip_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let result = json!({
            "timestamp": "2026-08-15T01:02:03.000Z",
            "marketSummary": "都市题材热度高",
            "recommendations": [ { "platform": "番茄小说" } ]
        });
        save_radar_scan(root, &result).await.unwrap();
        let file = root.join("radar").join("scan-2026-08-15T01-02-03-000Z.json");
        assert!(file.is_file());

        let history = load_radar_history(root).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["marketSummary"], "都市题材热度高");
        assert_eq!(history[0]["summaryPreview"], "都市题材热度高");

        // 第二次扫描 + 非扫描文件干扰 → 仍按 file 降序两条。
        save_radar_scan(root, &json!({ "timestamp": "2026-08-16T00:00:00.000Z", "marketSummary": "次日" }))
            .await
            .unwrap();
        std::fs::write(root.join("radar").join("notes.txt"), "").unwrap();
        let history = load_radar_history(root).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["file"], "scan-2026-08-16T00-00-00-000Z.json");
    }
}
