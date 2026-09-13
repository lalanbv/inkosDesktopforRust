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
    /// 检测自动改写环（111 号：inkos.json `detection` 节——enabled 才跑）。
    detection: Option<crate::models::project::DetectionConfig>,
    /// 通知通道（111 号：pause pipeline-error / diagnostic-alert webhook 事件）。
    notify_channels: Vec<crate::notify::NotifyChannel>,
    /// 质量门控（112 号：TS config.qualityGates——maxAuditRetries /
    /// pauseAfterConsecutiveFailures / retryTemperatureStep）。
    quality_gates: crate::models::project::QualityGates,
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
            detection: raw
                .as_ref()
                .and_then(|config| config.get("detection"))
                .and_then(|value| serde_json::from_value(value.clone()).ok()),
            notify_channels: crate::notify::parse_notify_channels(
                raw.as_ref().and_then(|config| config.get("notify")),
            ),
            quality_gates: raw
                .as_ref()
                .and_then(|config| config.get("qualityGates"))
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_default(),
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
            detection: None,
            notify_channels: Vec::new(),
            quality_gates: crate::models::project::QualityGates::default(),
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
    /// 失败维度聚类（111 号：bookId → dimension → count；≥3 → diagnostic-alert）。
    failure_dimensions: HashMap<String, HashMap<String, u32>>,
}

fn write_cycle_state() -> &'static Mutex<WriteCycleState> {
    static STATE: OnceLock<Mutex<WriteCycleState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(WriteCycleState::default()))
}

async fn write_one_chapter(
    runtime: &BooksRuntime,
    book_id: &str,
    temperature: Option<f64>,
) -> Result<(bool, u32, String, Vec<String>), String> {
    use crate::pipeline::write_next::{write_next_chapter, WriteNextConfig};
    crate::write_next_assembly!(runtime, agents, ctx);
    let config = WriteNextConfig::from_project(runtime.state.project_root()).await;
    let result = write_next_chapter(
        &runtime.state,
        &agents,
        &ctx,
        &config,
        book_id,
        None,
        temperature,
        None,
    )
    .await
    .map_err(|e| e.to_string())?;
    let success = result.status == "ready-for-review";
    // 失败维度聚类原料（TS auditResult.issues.map(category)）。
    let issue_categories: Vec<String> = result
        .audit_result
        .issues
        .iter()
        .map(|issue| issue.category.clone())
        .collect();
    Ok((success, result.chapter_number, result.status.to_string(), issue_categories))
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
    // 质量门控经 config.qualityGates（112 号；缺省与 TS QualityGatesSchema 默认一致）。
    let max_audit_retries = config.quality_gates.max_audit_retries;
    let pause_after_consecutive_failures = config.quality_gates.pause_after_consecutive_failures.max(1);
    let retry_temperature_step = config.quality_gates.retry_temperature_step;

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
            Some((0.7 + failures as f64 * retry_temperature_step).min(1.2))
        } else {
            None
        };

        // (成功位, 本章产物, 审计失败维度)——TS onChapterComplete 在成功与
        // 审计未过两条路径都带真实章号/状态回调（异常路径才只走 onError）。
        let (success, written, issue_categories) =
            match write_one_chapter(runtime, book_id, temperature).await {
                Ok((success, chapter_number, status, issue_categories)) => {
                    (success, Some((chapter_number, status)), issue_categories)
                }
                Err(error) => {
                    runtime.hub.broadcast(
                        "daemon:error",
                        &json!({ "bookId": book_id, "error": error }),
                    );
                    (false, None, Vec::new())
                }
            };

        if success {
            write_cycle_state().lock().unwrap().consecutive_failures.remove(book_id);
            let mut state = write_cycle_state().lock().unwrap();
            let today = today_key();
            let count = state.daily_counts.get(&today).copied().unwrap_or(0) + 1;
            state.daily_counts.retain(|key, _| key == &today);
            state.daily_counts.insert(today, count);
        }
        if let Some((chapter_number, _status)) = written.as_ref() {
            runtime.hub.broadcast(
                "daemon:chapter",
                &json!({ "bookId": book_id, "chapter": chapter_number, "status": _status }),
            );
            // 检测自动改写环（111 号：TS Scheduler.runDetection——成功审计后）。
            if let Some(detection) = &config.detection {
                if detection.enabled {
                    if let Err(error) =
                        run_detection(runtime, detection, book_id, *chapter_number).await
                    {
                        runtime.hub.broadcast(
                            "daemon:error",
                            &json!({ "bookId": book_id, "error": error }),
                        );
                    }
                }
            }
        }
        if success {
            continue;
        }

        let failures = {
            let mut state = write_cycle_state().lock().unwrap();
            let failures = state.consecutive_failures.entry(book_id.to_string()).or_insert(0);
            *failures += 1;
            *failures
        };
        // 失败维度聚类（111 号：任一维度 ≥3 → diagnostic-alert webhook）。
        let clustered: Vec<(String, u32)> = {
            let mut state = write_cycle_state().lock().unwrap();
            let dimensions = state
                .failure_dimensions
                .entry(book_id.to_string())
                .or_default();
            for category in &issue_categories {
                *dimensions.entry(category.clone()).or_insert(0) += 1;
            }
            dimensions
                .iter()
                .filter(|(_, count)| **count >= 3)
                .map(|(dimension, count)| (dimension.clone(), *count))
                .collect()
        };
        for (dimension, count) in clustered {
            crate::notify::dispatch_webhook_event(
                &config.notify_channels,
                &crate::notify::WebhookPayload {
                    event: "diagnostic-alert".to_string(),
                    book_id: book_id.to_string(),
                    chapter_number: written.as_ref().map(|(chapter, _)| *chapter),
                    timestamp: crate::utils::utc_time::utc_now_iso(),
                    data: Some(json!({ "dimension": dimension, "failureCount": count })),
                },
            )
            .await;
        }
        if failures >= pause_after_consecutive_failures {
            write_cycle_state().lock().unwrap().paused_books.insert(book_id.to_string());
            // pipeline-error webhook（111 号：TS handleAuditFailure 暂停分支）。
            let reason = format!(
                "{failures} consecutive audit failures (threshold: {pause_after_consecutive_failures})"
            );
            crate::notify::dispatch_webhook_event(
                &config.notify_channels,
                &crate::notify::WebhookPayload {
                    event: "pipeline-error".to_string(),
                    book_id: book_id.to_string(),
                    chapter_number: written
                        .as_ref()
                        .and_then(|(chapter, _)| (*chapter > 0).then_some(*chapter)),
                    timestamp: crate::utils::utc_time::utc_now_iso(),
                    data: Some(json!({ "reason": reason, "consecutiveFailures": failures })),
                },
            )
            .await;
        }
        if failures <= max_audit_retries && config.retry_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(config.retry_delay_ms)).await;
            let retry_temperature = Some((0.7 + failures as f64 * retry_temperature_step).min(1.2));
            match write_one_chapter(runtime, book_id, retry_temperature).await {
                Ok((true, chapter_number, status, _categories)) => {
                    write_cycle_state().lock().unwrap().consecutive_failures.remove(book_id);
                    runtime.hub.broadcast(
                        "daemon:chapter",
                        &json!({ "bookId": book_id, "chapter": chapter_number, "status": status }),
                    );
                }
                Ok((false, chapter_number, status, _categories)) => {
                    runtime.hub.broadcast(
                        "daemon:chapter",
                        &json!({ "bookId": book_id, "chapter": chapter_number, "status": status }),
                    );
                    break;
                }
                Err(_) => break,
            }
        } else {
            break;
        }
    }
}

/// `Scheduler.runDetection`（111 号）：读章文 → 单章检测 → 不过且 autoRewrite
/// → detect-and-rewrite 环（anti-detect 重写 + 重测 + history 落盘）。
async fn run_detection(
    runtime: &BooksRuntime,
    config: &crate::models::project::DetectionConfig,
    book_id: &str,
    chapter_number: u32,
) -> Result<(), String> {
    let book_dir = runtime.state.book_dir(book_id);
    // readChapterContent：chapters/NNNN*.md。
    let padded = format!("{:04}", chapter_number);
    let chapters_dir = book_dir.join("chapters");
    let mut entries = tokio::fs::read_dir(&chapters_dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut chapter_file: Option<String> = None;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&padded) && name.ends_with(".md") {
            chapter_file = Some(name);
            break;
        }
    }
    let Some(chapter_file) = chapter_file else {
        return Err(format!("chapter {chapter_number} file not found"));
    };
    let chapter_content =
        tokio::fs::read_to_string(chapters_dir.join(&chapter_file))
            .await
            .map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();
    let det = crate::pipeline::detection_runner::detect_chapter(&client, config, &chapter_content, chapter_number).await?;
    if !det.passed && config.auto_rewrite {
        let book = runtime.state.load_book_config(book_id).await.map_err(|e| e.to_string())?;
        let router = runtime.effective_router().await;
        let reviser_chat = crate::llm::agent_router::RoutedAgent {
            router: router.clone(),
            agent: "reviser",
        };
        let reviser_ports = crate::server::books_routes::AgentCtxPorts::new(runtime);
        crate::pipeline::detection_runner::detect_and_rewrite(
            &client,
            config,
            &reviser_chat,
            &reviser_ports.reviser_ctx(),
            &book_dir,
            &chapter_content,
            chapter_number,
            Some(&book.genre),
        )
        .await?;
    }
    Ok(())
}

fn scheduler_running() -> bool {
    scheduler_slot()
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|handle| handle.running.load(std::sync::atomic::Ordering::SeqCst))
}

async fn run_radar_scan_and_save(runtime: &BooksRuntime) -> Result<Value, String> {
    let result = crate::agents::radar::run_radar(&*runtime.effective_router().await).await?;
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
    // TS `loadRadarHistory`：目录缺失 catch 后返回空数组（200 {items:[]}）——
    // 此前 Err 上抛为 500，与 TS 契约偏差（286 号）。
    let mut entries = match tokio::fs::read_dir(&radar_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
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

/// G14a/335 号"选后再析"第一步：免费扫榜，不调 LLM、不落历史。
pub async fn post_radar_rankings(
    State(runtime): State<BooksRuntime>,
) -> impl IntoResponse {
    let _ = runtime;
    let rankings = crate::agents::radar::fetch_rankings().await;
    Json(json!({ "rankings": rankings })).into_response()
}

/// G14a/335 号第二步：勾选范围后分析（selection 可省 = 全量）。
#[derive(Debug, serde::Deserialize)]
pub struct RadarAnalyzeBody {
    #[serde(default)]
    pub selection: Option<crate::agents::radar::RadarSelection>,
}

pub async fn post_radar_analyze(
    State(runtime): State<BooksRuntime>,
    body: Option<Json<RadarAnalyzeBody>>,
) -> impl IntoResponse {
    runtime.hub.broadcast("radar:start", &json!({}));
    let selection = body.and_then(|Json(b)| b.selection);
    let result = async {
        let rankings = crate::agents::radar::fetch_rankings().await;
        crate::agents::radar::run_radar_analyze(
            &*runtime.effective_router().await,
            &rankings,
            selection.as_ref(),
        )
        .await
    }
    .await;
    match result {
        Ok(result) => {
            let value = serde_json::to_value(&result).unwrap_or(json!({}));
            if let Err(error) =
                save_radar_scan(runtime.state.project_root(), &value).await
            {
                runtime.hub.broadcast("radar:error", &json!({ "error": error }));
            }
            runtime.hub.broadcast("radar:complete", &json!({ "result": value }));
            (StatusCode::OK, Json(value)).into_response()
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

/// G5/351 号：统一拆书面——聚合 + 落盘 story/deconstruction/{name}.md。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeconstructBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub depth: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub top_characters: Option<usize>,
    #[serde(default)]
    pub chapters: Vec<crate::utils::deconstruction::DeconChapter>,
    #[serde(default)]
    pub publish: bool,
}

pub async fn post_deconstruct(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<DeconstructBody>,
) -> impl IntoResponse {
    if body.chapters.is_empty() {
        return flat_error(StatusCode::BAD_REQUEST, String::from("chapters is required"));
    }
    let name = body
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("decon-{}", crate::interaction::session::utc_now_ms()));
    let depth = body.depth.clone().unwrap_or_else(|| "full".into());
    let language = body.language.clone().unwrap_or_else(|| "zh".into());

    let result = crate::utils::deconstruction::build_deconstruction_export(
        &body.chapters,
        &depth,
        &language,
        body.top_characters,
    );
    let story_dir = runtime
        .state
        .project_root()
        .join("books")
        .join(&book_id)
        .join("story")
        .join("deconstruction");
    if let Err(error) = std::fs::create_dir_all(&story_dir) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let out_path = story_dir.join(format!("{name}.md"));
    if let Err(error) = std::fs::write(&out_path, &result.markdown) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }

    // 可选发布：产物写入项目材料池（.inkos/materials/），供参考资料绑定。
    let mut published_material_id: Option<String> = None;
    if body.publish {
        let material_id = format!("decon-{book_id}-{name}")
            .replace(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'), "-");
        let trimmed: String = material_id.chars().take(64).collect();
        let materials_dir = runtime.state.project_root().join(".inkos").join("materials");
        let _ = std::fs::create_dir_all(&materials_dir);
        let _ = std::fs::write(materials_dir.join(format!("{trimmed}.md")), &result.markdown);
        let manifest = json!({
            "id": trimmed,
            "title": name,
            "kind": "text",
            "purpose": "reference",
            "source": "deconstruction",
            "mimeType": "text/markdown",
            "markdownPath": format!(".inkos/materials/{trimmed}.md"),
        });
        let _ = std::fs::write(
            materials_dir.join(format!("{trimmed}.json")),
            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        );
        published_material_id = Some(trimmed);
    }

    let result_value = serde_json::to_value(&result).unwrap_or(json!(null));
    let mut payload = json!({
        "result": result_value,
        "path": format!("books/{book_id}/story/deconstruction/{name}.md"),
    });
    payload["publishedMaterialId"] = match &published_material_id {
        Some(id) => json!(id),
        None => Value::Null,
    };
    (StatusCode::OK, Json(payload)).into_response()
}

/// G6/354 号：方向候选批量生成（灵感卡 → LLM → 候选数组）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectionCandidatesBody {
    pub inspiration: crate::models::director::InspirationCard,
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default)]
    pub exclude_titles: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub asset_refs: Vec<AssetRef>,
}

/// R4/364 号：库资产引用（kind+id）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    pub kind: String,
    pub id: String,
}

pub async fn post_direction_candidates(
    State(runtime): State<BooksRuntime>,
    Json(body): Json<DirectionCandidatesBody>,
) -> impl IntoResponse {
    runtime.hub.broadcast("director:start", &json!({}));
    let count = body.count.unwrap_or(3).clamp(2, 5);
    // R4/364 号：可选挂载三库资产 guidance（refs 命中库资产 → 渲染参考块）。
    let mut asset_guidance: Option<String> = None;
    if !body.asset_refs.is_empty() {
        let language = crate::utils::language::infer_language(body.language.as_deref());
        let mut matched: Vec<crate::utils::asset_library::LibraryAsset> = Vec::new();
        for reference in body.asset_refs.iter().take(6) {
            let Some(kind) = crate::utils::asset_library::AssetKind::parse(&reference.kind) else { continue };
            let (assets, _) = crate::utils::asset_library::list_assets(&runtime.state.project_root(), kind)
                .unwrap_or_else(|_| (Vec::new(), false));
            if let Some(hit) = assets.iter().find(|asset| asset.id == reference.id) {
                matched.push(hit.clone());
            }
        }
        asset_guidance = crate::utils::asset_library::render_asset_guidance_block(&matched, language);
    }
    let prompt = crate::models::director::build_direction_candidates_prompt(
        &body.inspiration,
        count,
        &body.exclude_titles,
        body.language.as_deref(),
        asset_guidance.as_deref(),
    );
    let system = if body.language.as_deref() == Some("en") {
        "You are the story director."
    } else {
        "你是故事导演。"
    };
    let response = runtime
        .effective_router()
        .await
        .chat(
            "director",
            vec![
                crate::llm::provider::LLMMessage {
                    role: crate::llm::provider::LLMRole::System,
                    content: system.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                crate::llm::provider::LLMMessage {
                    role: crate::llm::provider::LLMRole::User,
                    content: prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            0.8,
            None,
        )
        .await;
    match response {
        Ok(completion) => {
            let directions = crate::models::director::parse_direction_candidates(
                &completion.content,
                &body.exclude_titles,
            );
            runtime
                .hub
                .broadcast("director:complete", &json!({ "count": directions.len() }));
            (StatusCode::OK, Json(json!({ "directions": directions }))).into_response()
        }
        Err(error) => {
            runtime
                .hub
                .broadcast("director:error", &json!({ "error": error }));
            flat_error(StatusCode::INTERNAL_SERVER_ERROR, error)
        }
    }
}

/// G6/353 号：导演会话读取（.inkos/director/{bookId}.json；缺省 null）+ 续跑建议。
pub async fn get_director(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let path = runtime
        .state
        .project_root()
        .join(".inkos")
        .join("director")
        .join(format!("{book_id}.json"));
    let session = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());
    let saved_chapters = runtime
        .state
        .load_chapter_index(&book_id)
        .await
        .map(|index| index.len() as i64)
        .unwrap_or(0);
    let resume_advice = crate::utils::resume_advice::format_resume_hint(saved_chapters, Some("en"));
    (
        StatusCode::OK,
        Json(json!({ "session": session, "savedChapters": saved_chapters, "resumeAdvice": resume_advice })),
    )
        .into_response()
}

/// G6/353 号：保存导演会话（合并写回；runMode 白名单校验）。
#[derive(Debug, serde::Deserialize)]
pub struct PutDirectorBody {
    #[serde(default)]
    pub patch: Value,
}

pub async fn put_director(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<PutDirectorBody>,
) -> impl IntoResponse {
    const ALLOWED: [&str; 6] = [
        "inspiration",
        "directions",
        "selectedDirection",
        "runMode",
        "stage",
        "plan",
    ];
    if let Some(mode) = body.patch.get("runMode").and_then(Value::as_str) {
        if !["ready-stop", "range", "full-book"].contains(&mode) {
            return flat_error(StatusCode::BAD_REQUEST, String::from("invalid runMode"));
        }
    }
    let dir = runtime.state.project_root().join(".inkos").join("director");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let path = dir.join(format!("{book_id}.json"));
    let mut session: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({ "bookId": book_id }));
    if let (Some(session_obj), Some(patch_obj)) = (session.as_object_mut(), body.patch.as_object()) {
        for (key, value) in patch_obj {
            if ALLOWED.contains(&key.as_str()) {
                session_obj.insert(key.clone(), value.clone());
            }
        }
        session_obj.insert("updatedAt".into(), json!(crate::interaction::session::utc_now_ms()));
        session_obj
            .entry("bookId".to_string())
            .or_insert_with(|| json!(book_id));
    }
    if let Err(error) = std::fs::write(&path, serde_json::to_string_pretty(&session).unwrap_or_default()) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    (StatusCode::OK, Json(json!({ "ok": true, "session": session }))).into_response()
}

/// G1/349 号：混合检索（FTS5 + 可选向量 RRF 融合；无 embedding 配置 → 纯 FTS5）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HybridSearchBody {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub k: Option<usize>,
    #[serde(default)]
    pub embedding: Option<crate::utils::semantic_retrieval::EmbeddingConfig>,
    #[serde(default, rename = "apiKeyEnv")]
    pub api_key_env: Option<String>,
}

pub async fn post_hybrid_search(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<HybridSearchBody>,
) -> impl IntoResponse {
    let query = body.query.trim().to_string();
    if query.is_empty() {
        return flat_error(StatusCode::BAD_REQUEST, String::from("query is required"));
    }
    let k = body.k.unwrap_or(10).clamp(1, 50);
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    let db_path = book_dir.join("story").join("memory.db");
    if !db_path.exists() {
        return (
            StatusCode::OK,
            Json(json!({ "fts": [], "semantic": [], "fused": [], "mode": "fts5-fallback" })),
        )
            .into_response();
    }

    let scope = "story-memory".to_string();
    let index = match crate::utils::local_search::LocalSearchIndex::new(db_path.to_str().unwrap_or("")) {
        Ok(index) => index,
        Err(error) => return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let fts_hits = index.search(
        &query,
        &crate::utils::local_search::SearchOptions {
            scope: &scope,
            kinds: &[],
            limit: k * 2,
        },
    );
    let fts_ranked: Vec<Value> = fts_hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            json!({ "id": hit.id, "rank": i + 1, "score": hit.score, "source": hit.source, "title": hit.title })
        })
        .collect();

    // embedding 可选：请求体带配置 → 语义召回；失败降级 fts5-fallback。
    let mut semantic_ranked: Vec<Value> = Vec::new();
    let mut mode = "fts5-fallback";
    if let Some(embedding) = &body.embedding {
        let semantic = async {
            let api_key = body
                .api_key_env
                .as_ref()
                .and_then(|env| std::env::var(env).ok())
                .unwrap_or_default();
            let query_vectors =
                crate::utils::semantic_retrieval::embed_batch(embedding, &[query.clone()], Some(&api_key))
                    .await?;
            let query_vector = query_vectors.first().ok_or("empty embedding")?;
            let db = crate::state::memory_db::MemoryDb::open(&book_dir).map_err(|e| e.to_string())?;
            let chunks = db.list_chunk_vectors().map_err(|e| e.to_string())?;
            let chunk_inputs: Vec<crate::utils::semantic_retrieval::ChunkVector> = chunks
                .iter()
                .map(|chunk| crate::utils::semantic_retrieval::ChunkVector {
                    id: chunk.chunk_id.clone(),
                    vector: chunk.vector.clone(),
                })
                .collect();
            let scored = crate::utils::semantic_retrieval::top_k_by_similarity(
                query_vector,
                &chunk_inputs,
                k * 2,
            );
            Ok::<Vec<Value>, String>(scored
                .iter()
                .enumerate()
                .map(|(i, hit)| json!({ "id": hit.id, "rank": i + 1 }))
                .collect())
        };
        match semantic.await {
            Ok(ranked) => {
                semantic_ranked = ranked;
                mode = "semantic";
            }
            Err(_) => mode = "fts5-fallback",
        }
    }

    let semantic_for_rrf: Vec<crate::utils::semantic_retrieval::RankedHit> = semantic_ranked
        .iter()
        .map(|entry| crate::utils::semantic_retrieval::RankedHit {
            id: entry["id"].as_str().unwrap_or_default().to_string(),
            rank: entry["rank"].as_i64().unwrap_or(0),
        })
        .collect();
    let fts_for_rrf: Vec<crate::utils::semantic_retrieval::RankedHit> = fts_ranked
        .iter()
        .map(|entry| crate::utils::semantic_retrieval::RankedHit {
            id: entry["id"].as_str().unwrap_or_default().to_string(),
            rank: entry["rank"].as_i64().unwrap_or(0),
        })
        .collect();
    let fused = crate::utils::semantic_retrieval::reciprocal_rank_fusion(&fts_for_rrf, &semantic_for_rrf, 60);

    (
        StatusCode::OK,
        Json(json!({
            "fts": fts_ranked,
            "semantic": semantic_ranked,
            "fused": fused,
            "mode": mode,
        })),
    )
        .into_response()
}

/// G16/346 号：读项目级任务路由（.inkos/task-routing.json；缺省 null）。
pub async fn get_task_routing(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let path = runtime.state.project_root().join(".inkos").join("task-routing.json");
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(routing) => (StatusCode::OK, Json(json!({ "routing": routing }))).into_response(),
            Err(_) => (StatusCode::OK, Json(json!({ "routing": Value::Null }))).into_response(),
        },
        Err(_) => (StatusCode::OK, Json(json!({ "routing": Value::Null }))).into_response(),
    }
}

/// G16/346 号：保存项目级任务路由（原子落盘 .inkos/task-routing.json）。
pub async fn put_task_routing(
    State(runtime): State<BooksRuntime>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let Some(routing) = body.get("routing") else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("routing is required"));
    };
    if !routing.is_object() {
        return flat_error(StatusCode::BAD_REQUEST, String::from("routing must be an object"));
    }
    let dir = runtime.state.project_root().join(".inkos");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let raw = match serde_json::to_string_pretty(routing) {
        Ok(raw) => raw,
        Err(error) => return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    match std::fs::write(dir.join("task-routing.json"), raw) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "routing": routing }))).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// G7b/343 号：名册候选确认卡（章摘要 characters 比对名册）。
pub async fn get_roster_candidates(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    let db_path = book_dir.join("story").join("memory.db");
    if !db_path.exists() {
        return (
            StatusCode::OK,
            Json(json!({ "cards": [], "candidates": [], "rosterExists": false, "currentChapter": 0 })),
        )
            .into_response();
    }
    let result = (|| {
        let db = crate::state::memory_db::MemoryDb::open(&book_dir)?;
        let summaries = db.get_summaries(1, 100_000)?;
        let current_chapter = summaries.iter().map(|s| s.chapter).max().map(|last| last + 1).unwrap_or(1);
        let candidates = crate::utils::entity_roster::extract_character_candidates(
            &summaries
                .iter()
                .map(|s| (s.chapter, s.characters.clone()))
                .collect::<Vec<_>>(),
        );
        let roster_path = book_dir.join("story").join("entity_roster.md");
        let (roster, roster_exists) = match std::fs::read_to_string(&roster_path) {
            Ok(raw) => (crate::utils::entity_roster::parse_entity_roster(&raw), true),
            Err(_) => (Vec::new(), false),
        };
        let cards = crate::utils::entity_roster::resolve_roster_candidates(&candidates, &roster);
        Ok::<_, crate::EngineError>(serde_json::json!({
            "cards": cards,
            "candidates": candidates,
            "rosterExists": roster_exists,
            "currentChapter": current_chapter
        }))
    })();
    match result {
        Ok(payload) => (StatusCode::OK, Json(payload)).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// G7b/343 号：确认三选动作 → 写回 story/entity_roster.md。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterConfirmBody {
    pub candidate: String,
    pub action: String,
    #[serde(default)]
    pub target_name: Option<String>,
    #[serde(default)]
    pub chapter: Option<i64>,
}

pub async fn post_roster_confirm(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<RosterConfirmBody>,
) -> impl IntoResponse {
    if body.candidate.trim().is_empty()
        || !matches!(body.action.as_str(), "new-entity" | "alias" | "typo")
    {
        return flat_error(
            StatusCode::BAD_REQUEST,
            String::from("candidate and action (new-entity|alias|typo) are required"),
        );
    }
    let story_dir = runtime
        .state
        .project_root()
        .join("books")
        .join(&book_id)
        .join("story");
    let roster_path = story_dir.join("entity_roster.md");
    let roster = match std::fs::read_to_string(&roster_path) {
        Ok(raw) => crate::utils::entity_roster::parse_entity_roster(&raw),
        Err(_) => Vec::new(),
    };
    let confirmation = crate::utils::entity_roster::RosterConfirmation {
        candidate: body.candidate.trim().to_string(),
        action: body.action.clone(),
        target_name: body.target_name.clone(),
        chapter: body.chapter,
    };
    let (roster, applied) = crate::utils::entity_roster::apply_roster_confirmation(&roster, &confirmation);
    if let Err(error) = std::fs::create_dir_all(&story_dir) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let rendered = crate::utils::entity_roster::render_entity_roster(&roster);
    if let Err(error) = std::fs::write(&roster_path, rendered) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let applied_value = serde_json::to_value(&applied).unwrap_or(json!(null));
    (StatusCode::OK, Json(json!({ "ok": true, "applied": applied_value }))).into_response()
}

/// G4/340 号：写法档案池列表（项目级 .inkos/style-profiles/）。
pub async fn list_style_profiles(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let dir = runtime.state.project_root().join(".inkos").join("style-profiles");
    let _ = std::fs::create_dir_all(&dir);
    let mut profiles: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if let Ok(raw) = std::fs::read_to_string(&path) {
                if let Ok(profile) = serde_json::from_str::<Value>(&raw) {
                    profiles.push(json!({ "name": name, "profile": profile }));
                }
            }
        }
    }
    (StatusCode::OK, Json(json!({ "profiles": profiles }))).into_response()
}

/// G4/340 号：保存写法档案（覆盖写，原子）。
#[derive(Debug, serde::Deserialize)]
pub struct SaveStyleProfileBody {
    pub name: String,
    pub profile: Value,
}

pub async fn save_style_profile(
    State(runtime): State<BooksRuntime>,
    Json(body): Json<SaveStyleProfileBody>,
) -> impl IntoResponse {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return flat_error(StatusCode::BAD_REQUEST, String::from("name is required"));
    }
    let dir = runtime.state.project_root().join(".inkos").join("style-profiles");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.json"));
    match serde_json::to_string_pretty(&body.profile) {
        Ok(raw) => match std::fs::write(&path, raw) {
            Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "name": name }))).into_response(),
            Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
        },
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// G4/340 号：读书级写法绑定（缺文件 → binding: null）。
pub async fn get_style_binding(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let path = runtime
        .state
        .project_root()
        .join("books")
        .join(&book_id)
        .join("story")
        .join("style_binding.json");
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(binding) => (StatusCode::OK, Json(json!({ "binding": binding }))).into_response(),
            Err(_) => (StatusCode::OK, Json(json!({ "binding": Value::Null }))).into_response(),
        },
        Err(_) => (StatusCode::OK, Json(json!({ "binding": Value::Null }))).into_response(),
    }
}

/// G4/340 号：保存书级写法绑定（校验档案存在；原子写）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveStyleBindingBody {
    pub profile_name: String,
    #[serde(default)]
    pub enabled_ids: Vec<String>,
    #[serde(default)]
    pub disabled_ids: Vec<String>,
    #[serde(default, rename = "maxGuidanceChars")]
    pub max_guidance_chars: Option<usize>,
}

pub async fn save_style_binding(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<SaveStyleBindingBody>,
) -> impl IntoResponse {
    let profile_name = body.profile_name.trim().to_string();
    if profile_name.is_empty() {
        return flat_error(StatusCode::BAD_REQUEST, String::from("profileName is required"));
    }
    let profile_path = runtime
        .state
        .project_root()
        .join(".inkos")
        .join("style-profiles")
        .join(format!("{profile_name}.json"));
    if !profile_path.exists() {
        return flat_error(StatusCode::NOT_FOUND, String::from("profile not found"));
    }
    let mut binding = json!({ "profileName": profile_name });
    if !body.enabled_ids.is_empty() {
        binding["enabledIds"] = json!(body.enabled_ids);
    }
    if !body.disabled_ids.is_empty() {
        binding["disabledIds"] = json!(body.disabled_ids);
    }
    if let Some(max) = body.max_guidance_chars {
        binding["maxGuidanceChars"] = json!(max);
    }
    let story_dir = runtime
        .state
        .project_root()
        .join("books")
        .join(&book_id)
        .join("story");
    if let Err(error) = std::fs::create_dir_all(&story_dir) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    let path = story_dir.join("style_binding.json");
    let raw = serde_json::to_string_pretty(&binding).unwrap_or_default();
    match std::fs::write(&path, raw) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "binding": binding }))).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// R3/361 号：质量趋势（review_metrics 沉淀 + 承诺紧迫度汇总）。
pub async fn get_quality_trend(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    let db_path = book_dir.join("story").join("memory.db");
    if !db_path.exists() {
        return (
            StatusCode::OK,
            Json(json!({
                "trend": { "points": [], "scoredChapters": 0, "averageScore": null, "failingChapters": [] },
                "urgency": [],
                "currentChapter": 0
            })),
        )
            .into_response();
    }
    let result = (|| async {
        let db = crate::state::memory_db::MemoryDb::open(&book_dir)?;
        let metrics = db.list_review_metrics()?;
        let rows: Vec<crate::utils::quality_trend::ReviewMetricRow> = metrics
            .iter()
            .map(|metric| crate::utils::quality_trend::ReviewMetricRow {
                chapter: metric.chapter,
                overall_score: metric.overall_score,
                passed: metric.passed,
                critical_count: metric.critical_count,
                warning_count: metric.warning_count,
                info_count: metric.info_count,
                recorded_at: String::new(),
            })
            .collect();
        let trend = crate::utils::quality_trend::build_quality_trend(&rows);
        let hooks = db.get_all_hooks()?;
        let current_chapter = metrics
            .iter()
            .map(|metric| metric.chapter)
            .max()
            .map(|last| last + 1)
            .unwrap_or(1);
        let target_chapters = runtime
            .state
            .load_book_config(&book_id)
            .await
            .ok()
            .map(|book| i64::from(book.target_chapters));
        let mut urgency: Vec<crate::utils::promise_ledger::PromiseUrgency> = hooks
            .iter()
            .filter(|hook| {
                !matches!(
                    hook.status.trim().to_lowercase().as_str(),
                    "resolved" | "closed" | "done" | "已回收" | "已解决"
                )
            })
            .map(|hook| {
                crate::utils::promise_ledger::PromiseHookInput {
                    hook_id: hook.hook_id.clone(),
                    start_chapter: hook.start_chapter,
                    status: hook.status.clone(),
                    last_advanced_chapter: hook.last_advanced_chapter,
                    expected_payoff: hook.expected_payoff.clone(),
                    notes: hook.notes.clone(),
                    core_hook: false,
                    // 紧迫度投影无 kind 字段（R23/396 号仅 timeline 透传）。
                    kind: None,
                }
            })
            .map(|input| {
                crate::utils::promise_ledger::resolve_promise_urgency(
                    &input,
                    current_chapter,
                    target_chapters,
                )
            })
            .collect();
        urgency.sort_by(|a, b| {
            b.urgency.cmp(&a.urgency).then_with(|| a.hook_id.cmp(&b.hook_id))
        });
        Ok::<_, crate::EngineError>(serde_json::json!({
            "trend": trend,
            "urgency": urgency,
            "currentChapter": current_chapter
        }))
    })();
    match result.await {
        Ok(payload) => (StatusCode::OK, Json(payload)).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// R2/359 号：张力曲线（读 chapter_summaries.md 真相源；缺分章自动跳过，曲线是展示层）。
pub async fn get_tension_curve(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    let summaries_path = book_dir.join("story").join("chapter_summaries.md");
    if !summaries_path.exists() {
        return (
            StatusCode::OK,
            Json(json!({
                "curve": { "points": [], "scoredChapters": 0, "unscoredChapters": 0 },
                "warnings": []
            })),
        )
            .into_response();
    }
    let result = (|| {
        let markdown = std::fs::read_to_string(&summaries_path)
            .map_err(|e| crate::EngineError::Io(e))?;
        let summaries = crate::utils::story_markdown::parse_chapter_summaries_markdown(&markdown);
        let rows: Vec<crate::utils::tension_curve::TensionRow> = summaries
            .iter()
            .map(|row| crate::utils::tension_curve::TensionRow {
                chapter: row.chapter,
                conflict_level: row.conflict_level,
                reveal_level: row.reveal_level,
            })
            .collect();
        let (curve, warnings) = crate::utils::tension_curve::analyze_tension_curve(
            &rows,
            crate::utils::tension_curve::TensionLanguage::Zh,
            &crate::utils::tension_curve::TENSION_WARNING_DEFAULTS,
        );
        Ok::<_, crate::EngineError>(serde_json::json!({
            "curve": curve,
            "warnings": warnings
        }))
    })();
    match result {
        Ok(payload) => (StatusCode::OK, Json(payload)).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// G10/338 号：承诺账本运营投影（时间线 + 节奏债 + 连续弱钩）。
pub async fn get_promises(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    let db_path = book_dir.join("story").join("memory.db");
    if !db_path.exists() {
        return (
            StatusCode::OK,
            Json(json!({
                "timeline": [],
                "pacingDebts": [],
                "weakRuns": { "runs": [], "longestRun": 0 },
                "currentChapter": 0
            })),
        )
            .into_response();
    }
    let result = (|| {
        let db = crate::state::memory_db::MemoryDb::open(&book_dir)?;
        let hooks = db.get_all_hooks()?;
        // R23/396 号：sqlite 投影不落 kind（358 号真相源单点），分类从
        // pending_hooks.md 台账第 14 列补齐——缺文件/缺列优雅降级为无 kind。
        let hooks_markdown = std::fs::read_to_string(book_dir.join("story").join("pending_hooks.md"))
            .unwrap_or_default();
        let mut kind_by_id: std::collections::HashMap<String, crate::models::runtime_state::HookKind> =
            std::collections::HashMap::new();
        for hook in crate::utils::story_markdown::parse_pending_hooks_markdown(&hooks_markdown) {
            if let Some(kind) = hook.kind {
                kind_by_id.insert(hook.hook_id.clone(), kind);
            }
        }
        let summaries = db.get_summaries(1, 100_000)?;
        let hook_inputs: Vec<crate::utils::promise_ledger::PromiseHookInput> = hooks
            .iter()
            .map(|hook| crate::utils::promise_ledger::PromiseHookInput {
                hook_id: hook.hook_id.clone(),
                start_chapter: hook.start_chapter,
                status: hook.status.clone(),
                last_advanced_chapter: hook.last_advanced_chapter,
                expected_payoff: hook.expected_payoff.clone(),
                notes: hook.notes.clone(),
                core_hook: false,
                kind: kind_by_id.get(&hook.hook_id).copied(),
            })
            .collect();
        let current_chapter = summaries
            .iter()
            .map(|summary| summary.chapter)
            .max()
            .map(|last| last + 1)
            .unwrap_or(1);
        let timeline = crate::utils::promise_ledger::build_promise_timeline(&hook_inputs, current_chapter);
        let pacing_debts = crate::utils::promise_ledger::detect_pacing_debts(&hook_inputs, current_chapter, None);
        let weak_rows: Vec<crate::utils::promise_ledger::WeakHookSummaryRow> = summaries
            .iter()
            .map(|summary| crate::utils::promise_ledger::WeakHookSummaryRow {
                chapter: summary.chapter,
                hook_activity: summary.hook_activity.clone(),
            })
            .collect();
        let weak_runs = crate::utils::promise_ledger::detect_weak_hook_runs(&weak_rows, None);
        Ok::<_, crate::EngineError>(serde_json::json!({
            "timeline": timeline,
            "pacingDebts": pacing_debts,
            "weakRuns": weak_runs,
            "currentChapter": current_chapter
        }))
    })();
    match result {
        Ok(payload) => (StatusCode::OK, Json(payload)).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// G3/337 号：质量债务清单（?status=open|deferred|resolved 过滤；缺省全量）。
pub async fn get_quality_debts(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    let db_path = book_dir.join("story").join("memory.db");
    if !db_path.exists() {
        return (StatusCode::OK, Json(json!({ "debts": [] }))).into_response();
    }
    match crate::state::memory_db::MemoryDb::open(&book_dir) {
        Ok(db) => match db.list_debts(None, params.get("status").map(String::as_str)) {
            Ok(debts) => {
                let value = serde_json::to_value(&debts).unwrap_or(json!([]));
                (StatusCode::OK, Json(json!({ "debts": value }))).into_response()
            }
            Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
        },
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

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
    // G1/349 号：检索层健康——embedding 配置存在 → semantic 能力，否则 FTS5 降级。
    let embedding_configured = std::fs::read_to_string(root.join("inkos.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|config| {
            config
                .get("llm")
                .and_then(|llm| llm.get("embedding").map(|e| e.is_object()))
        })
        .unwrap_or(false);
    checks["retrieval"] = json!({
        "mode": if embedding_configured { "semantic" } else { "fts5-fallback" },
        "embeddingConfigured": embedding_configured,
        // R12/381 号：向量引擎标注——rusqlite 未启用 load_extension feature，
        // 探测恒不可用 → memory-cosine（380 号决策表安全回退路径）。
        "vectorEngine": "memory-cosine",
        "vecExtensionAvailable": false,
    });
    // 195 号：书籍级写作阻塞预警——最新章 state-degraded 会让下一章 write-next
    // 直接报错（PendingStateRepair），此前 doctor 不预警，用户只能等写作失败
    // 才知道。detail 文案由客户端按 kind/chapter 组装（双语），服务端只出结构。
    let mut book_issues: Vec<serde_json::Value> = Vec::new();
    for book_id in &books {
        let Ok(index) = runtime.state.load_chapter_index(book_id).await else {
            continue;
        };
        let Some(latest) = index.iter().max_by_key(|meta| meta.number) else {
            continue;
        };
        if latest.status == crate::models::chapter::ChapterStatus::StateDegraded {
            let title = runtime
                .state
                .load_book_config(book_id)
                .await
                .map(|book| book.title)
                .unwrap_or_else(|_| book_id.clone());
            book_issues.push(json!({
                "bookId": book_id,
                "title": title,
                "kind": "state-degraded",
                "chapter": latest.number,
            }));
        }
    }
    checks["bookIssues"] = json!(book_issues);
    // LLM 连通探测（99 号对齐 TS probeServiceCapabilities 主干）：models
    // GET → 不可达时 chat 深链（preferred stream → 空/失败回退非 stream），
    // 9 秒总预算（DOCTOR_LLM_PROBE_BUDGET_MS；慢/限流上游按未连接上报）。
    let probe = tokio::time::timeout(Duration::from_secs(9), async {
        let endpoint = runtime.effective_router().await.resolve("radar");
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
        // 深链回退：候选 = 端点模型；计划 = llm.apiFormat × llm.stream 的
        // preferred 分支（TS buildProbePlans——106 号起含 responses 维度）。
        let llm_config = crate::server::project_config_routes::load_raw_config(root)
            .await
            .and_then(|config| config.get("llm").cloned());
        let preferred_stream = llm_config
            .as_ref()
            .and_then(|llm| llm.get("stream").and_then(serde_json::Value::as_bool));
        let preferred_api_format = llm_config.as_ref().and_then(|llm| {
            llm.get("apiFormat").and_then(serde_json::Value::as_str).and_then(|value| match value {
                "responses" => Some(crate::llm::providers::TransportApiFormat::Responses),
                "chat" => Some(crate::llm::providers::TransportApiFormat::Chat),
                _ => None,
            })
        });
        for (plan_api_format, stream) in
            crate::server::service_routes::build_probe_plans(preferred_api_format, preferred_stream)
        {
            if crate::server::service_routes::minimal_chat_probe(
                &base,
                &endpoint.api_key,
                &endpoint.model,
                stream,
                plan_api_format,
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
    use crate::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use crate::pipeline::merged_audit::RevisionGate;
    use crate::server::books_routes::BooksRuntime;
    use crate::server::sse::BroadcastHub;
    use crate::state::manager::StateManager;
    use axum::routing::get;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn runtime_for(root: &std::path::Path) -> BooksRuntime {
        BooksRuntime {
            hub: Arc::new(BroadcastHub::new()),
            state: Arc::new(StateManager::new(root)),
            router: Arc::new(AgentRouter::new(
                LlmEndpointConfig {
                    base_url: "http://127.0.0.1:9".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    max_tokens: 1024,
                    extra_headers: Default::default(),
                },
                Default::default(),
            )),
            builtin_genres_dir: root.to_path_buf(),
            revision_gate: RevisionGate::default(),
        }
    }

    fn book_fixture(root: &std::path::Path, index_json: &str) {
        let book = root.join("books").join("b1");
        std::fs::create_dir_all(book.join("chapters")).unwrap();
        std::fs::write(
            book.join("book.json"),
            r#"{"id":"b1","title":"测试书","platform":"other","genre":"other","status":"active","targetChapters":10,"chapterWordCount":3000,"language":"zh","createdAt":"","updatedAt":""}"#,
        )
        .unwrap();
        std::fs::write(book.join("chapters").join("index.json"), index_json).unwrap();
    }

    /// 195 号：最新章 state-degraded → doctor 预警将阻塞下一章写作。
    #[tokio::test]
    async fn doctor_flags_state_degraded_latest_chapter() {
        let dir = tempfile::tempdir().unwrap();
        book_fixture(
            dir.path(),
            r#"[
                {"number":1,"title":"风起","status":"approved","wordCount":100,"createdAt":"","updatedAt":""},
                {"number":2,"title":"云涌","status":"state-degraded","wordCount":90,"createdAt":"","updatedAt":""}
            ]"#,
        );
        let app = axum::Router::new()
            .route("/api/v1/doctor", get(get_doctor))
            .with_state(runtime_for(dir.path()));
        let response = app
            .oneshot(axum::http::Request::builder().method("GET").uri("/api/v1/doctor").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let issues = parsed["bookIssues"].as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["bookId"], "b1");
        assert_eq!(issues[0]["kind"], "state-degraded");
        assert_eq!(issues[0]["chapter"], 2);
        assert_eq!(issues[0]["title"], "测试书");
    }

    #[tokio::test]
    async fn doctor_reports_no_issues_for_healthy_books() {
        let dir = tempfile::tempdir().unwrap();
        book_fixture(
            dir.path(),
            r#"[
                {"number":1,"title":"风起","status":"ready-for-review","wordCount":100,"createdAt":"","updatedAt":""}
            ]"#,
        );
        let app = axum::Router::new()
            .route("/api/v1/doctor", get(get_doctor))
            .with_state(runtime_for(dir.path()));
        let response = app
            .oneshot(axum::http::Request::builder().method("GET").uri("/api/v1/doctor").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["bookIssues"].as_array().unwrap().len(), 0);
    }

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

    /// 286 号：radar 目录缺失 → 空数组（TS catch 返回 []，200 {items:[]}），
    /// 此前 Err 上抛为 500 契约偏差。
    #[tokio::test]
    async fn radar_history_missing_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let history = load_radar_history(dir.path()).await.unwrap();
        assert!(history.is_empty());
    }
}

// ── R4/363 号：三库资产（项目级 .inkos/asset-library/{kind}.json；种子兜底）──

fn parse_library_kind(raw: &str) -> Option<crate::utils::asset_library::AssetKind> {
    crate::utils::asset_library::AssetKind::parse(raw)
}

pub async fn get_asset_library(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(kind_raw): axum::extract::Path<String>,
) -> impl IntoResponse {
    let Some(kind) = parse_library_kind(&kind_raw) else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid kind"));
    };
    match crate::utils::asset_library::list_assets(&runtime.state.project_root(), kind) {
        Ok((assets, seeded)) => (
            StatusCode::OK,
            Json(json!({ "kind": kind.as_str(), "assets": assets, "seeded": seeded })),
        )
            .into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct UpsertAssetBody {
    pub asset: Value,
}

pub async fn put_asset_library_asset(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(kind_raw): axum::extract::Path<String>,
    Json(body): Json<UpsertAssetBody>,
) -> impl IntoResponse {
    let Some(kind) = parse_library_kind(&kind_raw) else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid kind"));
    };
    let outcome = crate::utils::asset_library::upsert_asset(
        &runtime.state.project_root(),
        kind,
        &body.asset,
    );
    if !outcome.errors.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "errors": outcome.errors }))).into_response();
    }
    (StatusCode::OK, Json(serde_json::to_value(&outcome).unwrap_or_default())).into_response()
}

pub async fn delete_asset_library_asset(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path((kind_raw, id)): axum::extract::Path<(String, String)>,
) -> impl IntoResponse {
    let Some(kind) = parse_library_kind(&kind_raw) else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid kind"));
    };
    match crate::utils::asset_library::delete_asset(&runtime.state.project_root(), kind, &id) {
        Ok((true, _, assets)) => (StatusCode::OK, Json(json!({ "ok": true, "assets": assets }))).into_response(),
        Ok((false, Some(reason), _)) => {
            let status = if reason.contains("builtin seeds") { StatusCode::BAD_REQUEST } else { StatusCode::NOT_FOUND };
            flat_error(status, reason)
        }
        Ok((false, None, _)) => flat_error(StatusCode::NOT_FOUND, String::from("asset not found")),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error),
    }
}

pub async fn export_asset_library(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(kind_raw): axum::extract::Path<String>,
) -> impl IntoResponse {
    let Some(kind) = parse_library_kind(&kind_raw) else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid kind"));
    };
    let (assets, _) = match crate::utils::asset_library::list_assets(&runtime.state.project_root(), kind) {
        Ok(snapshot) => snapshot,
        Err(error) => return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let body = crate::utils::asset_library::build_asset_library_export(&assets);
    (
        StatusCode::OK,
        [
            ("Content-Type", "application/json; charset=utf-8"),
            ("Content-Disposition", Box::leak(format!("attachment; filename=\"asset-library-{kind_raw}.json\"").into_boxed_str())),
        ],
        body,
    )
        .into_response()
}

pub async fn import_asset_library(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(kind_raw): axum::extract::Path<String>,
    body: String,
) -> impl IntoResponse {
    let Some(kind) = parse_library_kind(&kind_raw) else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid kind"));
    };
    let parsed = crate::utils::asset_library::parse_asset_library_import(&body);
    if parsed.assets.is_empty() && !parsed.errors.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "errors": parsed.errors }))).into_response();
    }
    let (existing, _) = match crate::utils::asset_library::list_assets(&runtime.state.project_root(), kind) {
        Ok(snapshot) => snapshot,
        Err(error) => return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let merged = crate::utils::asset_library::merge_asset_library(&existing, &parsed.assets);
    if let Err(error) = crate::utils::asset_library::save_assets(&runtime.state.project_root(), kind, &merged.merged) {
        return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }
    (
        StatusCode::OK,
        Json(json!({
            "added": merged.added,
            "skipped": merged.skipped,
            "overwritten": merged.overwritten,
            "errors": parsed.errors,
            "assets": merged.merged,
        })),
    )
        .into_response()
}

/// R4/364 号：库资产两段式采用（通用样本≠本书世界——写入本书材料池，进 G1 召回链）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptLibraryAssetsBody {
    #[serde(default)]
    pub refs: Vec<AssetRef>,
    #[serde(default)]
    pub language: Option<String>,
}

pub async fn adopt_library_assets(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<AdoptLibraryAssetsBody>,
) -> impl IntoResponse {
    if body.refs.is_empty() {
        return flat_error(StatusCode::BAD_REQUEST, String::from("refs are required"));
    }
    let language = crate::utils::language::infer_language(body.language.as_deref());
    let materials_dir = runtime.state.project_root().join(".inkos").join("materials");
    let _ = std::fs::create_dir_all(&materials_dir);
    let mut published: Vec<Value> = Vec::new();
    for reference in body.refs.iter().take(12) {
        let Some(kind) = crate::utils::asset_library::AssetKind::parse(&reference.kind) else { continue };
        let (assets, _) = crate::utils::asset_library::list_assets(&runtime.state.project_root(), kind)
            .unwrap_or_else(|_| (Vec::new(), false));
        let Some(asset) = assets.iter().find(|asset| asset.id == reference.id) else { continue };
        let header = crate::utils::asset_library::render_adoption_header(asset, &book_id, language);
        let samples_block = if asset.samples.is_empty() {
            String::new()
        } else {
            let items: Vec<String> = asset.samples.iter().map(|sample| format!("- {sample}")).collect();
            format!("\n## Samples\n\n{}\n", items.join("\n"))
        };
        let markdown = format!("{header}\n{}\n{samples_block}", asset.body);
        let safe: String = format!("lib-{}-{}-{}", reference.kind, asset.id, book_id)
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .take(80)
            .collect();
        let material_id = safe;
        if let Err(error) = std::fs::write(materials_dir.join(format!("{material_id}.md")), &markdown) {
            return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
        }
        let manifest = json!({
            "id": material_id,
            "title": format!("{}（库资产）", asset.name),
            "kind": "text",
            "purpose": "reference",
            "source": "library",
            "mimeType": "text/markdown",
            "markdownPath": format!(".inkos/materials/{material_id}.md"),
        });
        if let Err(error) = std::fs::write(
            materials_dir.join(format!("{material_id}.json")),
            serde_json::to_string_pretty(&manifest).unwrap_or_default(),
        ) {
            return flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
        }
        published.push(json!({ "materialId": material_id, "kind": reference.kind, "id": asset.id }));
    }
    (StatusCode::OK, Json(json!({ "published": published }))).into_response()
}

// ── R5/366 号：书级反AI规则 + G13 经验条目 ──

fn read_rules_file(book_dir: &Path, file: &str, key: &str) -> Value {
    let raw = std::fs::read_to_string(book_dir.join("story").join(file)).unwrap_or_default();
    let parsed: Value = serde_json::from_str(&raw).unwrap_or(json!({}));
    let items = parsed.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut map = serde_json::Map::new();
    map.insert(key.to_string(), Value::Array(items));
    Value::Object(map)
}

pub async fn get_anti_ai_rules(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    // R22/392 号：GET 种子兜底——文件缺失/损坏/rules 键缺失 → 内置种子
    // （seeded=true，内存态不落盘）；显式空规则 = 用户选择，不回填。
    let rules_array = std::fs::read_to_string(book_dir.join("story").join("anti_ai_rules.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|parsed| parsed.get("rules").and_then(Value::as_array).cloned());
    match rules_array {
        Some(rules) => (
            StatusCode::OK,
            Json(json!({ "rules": rules, "seeded": false })),
        )
            .into_response(),
        None => (
            StatusCode::OK,
            Json(json!({
                "rules": crate::utils::rule_experience_engine::anti_ai_rule_seeds(),
                "seeded": true,
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct PutRulesBody {
    pub rules: Vec<Value>,
}

pub async fn put_anti_ai_rules(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<PutRulesBody>,
) -> impl IntoResponse {
    let mut valid: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (index, raw) in body.rules.iter().enumerate() {
        let result = crate::utils::rule_experience_engine::validate_anti_ai_rule(raw);
        match result.rule {
            Some(rule) => valid.push(serde_json::to_value(&rule).unwrap_or_default()),
            None => {
                for message in result.errors {
                    errors.push(format!("rules[{index}] {message}"));
                }
            }
        }
    }
    if !errors.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "errors": errors }))).into_response();
    }
    let story_dir = runtime.state.project_root().join("books").join(&book_id).join("story");
    let _ = std::fs::create_dir_all(&story_dir);
    match std::fs::write(
        story_dir.join("anti_ai_rules.json"),
        serde_json::to_string_pretty(&json!({ "version": 1, "rules": valid })).unwrap_or_default(),
    ) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "rules": valid }))).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

pub async fn get_experience_entries(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let book_dir = runtime.state.project_root().join("books").join(&book_id);
    (StatusCode::OK, Json(read_rules_file(&book_dir, "experience.json", "entries"))).into_response()
}

#[derive(Debug, serde::Deserialize)]
pub struct PutExperienceBody {
    pub entries: Vec<crate::utils::rule_experience_engine::ExperienceEntry>,
}

pub async fn put_experience_entries(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(book_id): axum::extract::Path<String>,
    Json(body): Json<PutExperienceBody>,
) -> impl IntoResponse {
    let path = runtime
        .state
        .project_root()
        .join("books")
        .join(&book_id)
        .join("story")
        .join("experience.json");
    let existing: Vec<crate::utils::rule_experience_engine::ExperienceEntry> =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|parsed| {
                parsed
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| serde_json::from_value(item.clone()).ok())
                            .collect()
                    })
            })
            .unwrap_or_default();
    let merged = crate::utils::rule_experience_engine::merge_experience_entries(&existing, &body.entries);
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    match std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({ "version": 1, "entries": merged.merged })).unwrap_or_default(),
    ) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "entries": merged.merged, "added": merged.added })),
        )
            .into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

pub async fn delete_experience_entry(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path((book_id, entry_id)): axum::extract::Path<(String, String)>,
) -> impl IntoResponse {
    let path = runtime
        .state
        .project_root()
        .join("books")
        .join(&book_id)
        .join("story")
        .join("experience.json");
    let entries: Vec<crate::utils::rule_experience_engine::ExperienceEntry> =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|parsed| {
                parsed
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| serde_json::from_value(item.clone()).ok())
                            .collect()
                    })
            })
            .unwrap_or_default();
    let remaining: Vec<crate::utils::rule_experience_engine::ExperienceEntry> = entries
        .iter()
        .filter(|entry| entry.id != entry_id)
        .cloned()
        .collect();
    if remaining.len() == entries.len() {
        return flat_error(StatusCode::NOT_FOUND, format!("entry not found: {entry_id}"));
    }
    match std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({ "version": 1, "entries": remaining })).unwrap_or_default(),
    ) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "entries": remaining }))).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

/// R13/382 号：写作数据聚合（产出节奏/通过率/token 成本；纯索引聚合）。
pub async fn get_writing_stats(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root();
    let mut rows: Vec<(String, i64, String, u64, Option<u64>)> = Vec::new();
    let books_dir = root.join("books");
    if let Ok(entries) = std::fs::read_dir(&books_dir) {
        for entry in entries.flatten() {
            let book_dir = entry.path();
            let index_path = book_dir.join("story").join("chapter-index.json");
            let Ok(raw) = std::fs::read_to_string(&index_path) else { continue };
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else { continue };
            let Some(items) = parsed.as_array().or_else(|| parsed.get("chapters").and_then(Value::as_array)) else { continue };
            for item in items {
                rows.push((
                    item.get("updatedAt").and_then(Value::as_str).unwrap_or_default().to_string(),
                    item.get("wordCount").and_then(Value::as_i64).unwrap_or(0),
                    item.get("status").and_then(Value::as_str).unwrap_or_default().to_string(),
                    item.get("totalTokens").and_then(Value::as_u64).unwrap_or(0),
                    item.get("tokenUsage").and_then(|t| t.get("totalTokens")).and_then(Value::as_u64),
                ));
            }
        }
    }
    // TS 端点形状对齐：单行简报（聚合在客户端由同一 writing-stats 纯函数完成）。
    (StatusCode::OK, Json(json!({ "rows": rows }))).into_response()
}

/// R15/384 号：市场包导入预览（零落盘——解析+预估新增/覆盖/跳过）。
pub async fn preview_asset_library_import(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(kind_raw): axum::extract::Path<String>,
    body: String,
) -> impl IntoResponse {
    let Some(kind) = crate::utils::asset_library::AssetKind::parse(&kind_raw) else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid kind"));
    };
    let parsed = crate::utils::asset_library::parse_asset_library_import(&body);
    if parsed.assets.is_empty() && !parsed.errors.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "errors": parsed.errors }))).into_response();
    }
    let (existing, _) = crate::utils::asset_library::list_assets(&runtime.state.project_root(), kind)
        .unwrap_or_else(|_| (Vec::new(), false));
    let merged = crate::utils::asset_library::merge_asset_library(&existing, &parsed.assets);
    let samples: Vec<serde_json::Value> = parsed
        .assets
        .iter()
        .map(|asset| json!({ "id": asset.id, "name": asset.name, "kind": asset.kind.as_str() }))
        .collect();
    (
        StatusCode::OK,
        Json(json!({
            "added": merged.added,
            "overwritten": merged.overwritten,
            "skipped": merged.skipped,
            "errors": parsed.errors,
            "samples": samples,
        })),
    )
        .into_response()
}

// ── R20/390 号：系列正典共享（项目级 .inkos/series/{seriesId}.json；全局级不带 bookId）──

pub async fn get_series_canon(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(series_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if !crate::utils::series_canon::is_valid_series_id(&series_id) {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid series id"));
    }
    let file = crate::utils::series_canon::load_series_canon(&runtime.state.project_root(), &series_id)
        .unwrap_or(crate::utils::series_canon::SeriesCanonFile {
            version: crate::utils::series_canon::SERIES_CANON_VERSION,
            series_id: series_id.clone(),
            entries: Vec::new(),
        });
    (
        StatusCode::OK,
        Json(serde_json::to_value(&file).unwrap_or_default()),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
pub struct PutSeriesCanonBody {
    pub entries: Value,
}

pub async fn put_series_canon(
    State(runtime): State<BooksRuntime>,
    axum::extract::Path(series_id): axum::extract::Path<String>,
    Json(body): Json<PutSeriesCanonBody>,
) -> impl IntoResponse {
    if !crate::utils::series_canon::is_valid_series_id(&series_id) {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid series id"));
    }
    if !body.entries.is_array() {
        return flat_error(StatusCode::BAD_REQUEST, String::from("entries array required"));
    }
    let parsed = crate::utils::series_canon::parse_series_canon_file(&json!({
        "version": crate::utils::series_canon::SERIES_CANON_VERSION,
        "seriesId": series_id,
        "entries": body.entries,
    }));
    let Some(parsed) = parsed else {
        return flat_error(StatusCode::BAD_REQUEST, String::from("invalid series canon payload"));
    };
    let entry_count = parsed.entries.len();
    match crate::utils::series_canon::save_series_canon(&runtime.state.project_root(), &parsed) {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true, "entries": entry_count }))).into_response(),
        Err(error) => flat_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}
