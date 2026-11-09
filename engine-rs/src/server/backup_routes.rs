//! R18/386 号备份双端点 Rust 移植（443 号）。
//!
//! 386 备忘 §4 曾书面决策「备份为纯本地 UI 面，仅 TS 承担」——164 号 Rust
//! 成为桌面默认引擎后，该 UI 面在默认路径上整体死亡（导出/导入均 404），
//! 决策前提失效；443 号推翻并移植，线格式与 TS `server.ts` 逐字对齐：
//!
//! - `GET /api/v1/backup/export`：staging 聚合 `books/` + `inkos.json` +
//!   `.inkos/`（secrets 默认排除，`includeSecrets=1` 显式含）+ `prompt/` +
//!   `backup-manifest.json` → `inkos-backup/` 前缀 tar.gz 下载。
//!   `sessions/` 默认排除（`includeSessions` 仅记录进 manifest scope）。
//! - `POST /api/v1/backup/import[?confirm=1]`：两段式。预览返回
//!   `{preview:{fileCount,overwriting,newFiles}}`；确认写回前自动快照
//!   覆盖项至 `backups/pre-restore-<ms>.tar`（回滚保险）。
//!
//! 安全三则（与 TS `tar-read.ts` 同语义）：条目名含 `..` 或绝对路径拒绝；
//! 仅接受普通文件/目录条目；总解压字节数上限 512MB。manifest 先验
//! （version=1 + generator 前缀 `inkos-desktop`）。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::json;
use std::collections::HashMap;

use crate::server::books_routes::BooksRuntime;

// 938/939 号：备份域对照引擎数据目录全集补缺——`works`（work 域创作
// 数据）+`translations`（翻译工程）+`genres`（用户自定义题材）+`skills`
// （项目技能）+`radar`（radar 数据）+`worlds`（影游项目）；缺失即对应
// 用户数据不进备份（恢复后丢失）。import 为通用 staging 复制天然支持
// 全部条目，恢复端无需改动。assets 素材库在 .inkos/asset-library 内
// （已覆盖）；backups/（pre-restore 快照）与 inkos.log 不自包含。
const BACKUP_ROOT_ENTRIES: [&str; 10] = [
    "books",
    "inkos.json",
    ".inkos",
    "prompt",
    "works",
    "translations",
    "genres",
    "skills",
    "radar",
    "worlds",
];
const PACKAGE_ROOT_NAME: &str = "inkos-backup";
const MANIFEST_NAME: &str = "backup-manifest.json";
const TAR_MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

struct ArchiveEntry {
    name: String,
    bytes: Vec<u8>,
}

fn is_existing(path: &Path) -> bool {
    std::fs::metadata(path).is_ok()
}

/// 955 号：manifest 条目判定（包根前缀名或裸名；TS `isManifestEntry` 同语义）。
/// 零分配比较——954 前写回循环逐条目 `format!` 拼前缀名比对。
fn is_manifest_entry(name: &str) -> bool {
    name == MANIFEST_NAME
        || name
            .strip_prefix(PACKAGE_ROOT_NAME)
            .and_then(|rest| rest.strip_prefix('/'))
            == Some(MANIFEST_NAME)
}

/// 递归复制（.inkos 拆层用）；符号链接按其目标文件/目录复制。
fn copy_recursive(source: &Path, target: &Path) -> std::io::Result<()> {
    let meta = std::fs::metadata(source)?;
    if meta.is_dir() {
        std::fs::create_dir_all(target)?;
        for child in std::fs::read_dir(source)? {
            let child = child?;
            copy_recursive(&child.path(), &target.join(child.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, target)?;
        Ok(())
    }
}

/// staging 相对文件清单（跳 .DS_Store；排序与 TS localeCompare 对齐为字节序，
/// 对 ASCII 路径等价——项目路径规范为 snake_case/中文无重音差异）。
fn list_archive_files(dir: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    let mut children: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    children.sort_by_key(|child| child.file_name());
    for child in children {
        let name = child.file_name().to_string_lossy().to_string();
        if name == ".DS_Store" {
            continue;
        }
        let relative = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let path = dir.join(&name);
        if path.is_dir() {
            list_archive_files(&path, &relative, out)?;
        } else if path.is_file() {
            out.push(relative);
        }
    }
    Ok(())
}

/// 948/949 号：打包前 per-work source_sync 快照补齐（幂等，accept=false 只
/// 快照不 accept）——防「手工放置/启动竞态」下 works 数据未同步即打包的完整
/// 性窗口（947 端到端验证发现备案）。949 号：只遍历目录条目——.DS_Store 等
/// 杂散文件不是 work，跳过且不告警（list_archive_files 同族先例）；逐 work
/// 隔离——单个 work sync 失败只 warn，不中止其余 work 的补齐（与 TS 对齐）。
async fn sync_works_for_backup(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root.join("works")) else {
        return;
    };
    for entry in entries.flatten() {
        let is_directory = entry.file_type().map(|file_type| file_type.is_dir()).unwrap_or(false);
        if !is_directory {
            continue;
        }
        let work_id = entry.file_name().to_string_lossy().to_string();
        let synced = crate::harness::source_sync::sync_work_source_artifacts(
            crate::harness::source_sync::SyncWorkSourceInput {
                project_root: root,
                work_id: &work_id,
                accept: false,
                accept_paths: None,
                episode_id: None,
                updated_at: None,
                writes: Vec::new(),
                title: None,
                status: None,
            },
        )
        .await;
        if let Err(error) = synced {
            tracing::warn!("[backup] works/{work_id} source sync 失败（不阻断备份）: {error}");
        }
    }
}

/// `GET /api/v1/backup/export`——全量 tar.gz 下载。
pub async fn export(
    State(runtime): State<BooksRuntime>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let include_secrets = params.get("includeSecrets").map(|v| v == "1").unwrap_or(false);
    let include_sessions = params.get("includeSessions").map(|v| v == "1").unwrap_or(false);
    let root = runtime.state.project_root().to_path_buf();
    // 763 号：整书 staging 复制 + tar + gzip 是秒级阻塞工作，移出 runtime
    // 线程（run_export 本体保持同步签名——725 号 bench 契约面语义不变）。
    sync_works_for_backup(&root).await;
    let result = tokio::task::spawn_blocking(move || run_export(&root, include_secrets, include_sessions))
        .await;
    match result {
        Ok(Ok(bytes)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/gzip"),
                (header::CONTENT_DISPOSITION, "attachment; filename=\"inkos-backup.tar.gz\""),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

// 725 号 0GC：pub 化供 bench 消费（hot_paths backup_export 组基线——staging
// 复制为 443 号契约面语义保持，gzip level 6 为合理档位不做压缩级别调参）。
pub fn run_export(root: &Path, include_secrets: bool, include_sessions: bool) -> std::io::Result<Vec<u8>> {
    // 940 号：includeSessions=1 真正打包 sessions/（原形态参数仅进 manifest
    // scope、打包范围静默忽略=参数骗人；sessions/ 为会话 JSONL 用户创作上
    // 下文）。默认仍排除。
    let staging = root.join(".inkos-backup-staging");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let copy_result = (|| -> std::io::Result<()> {
        for entry in BACKUP_ROOT_ENTRIES {
            let source = root.join(entry);
            if !is_existing(&source) {
                continue;
            }
            if entry != ".inkos" {
                copy_recursive(&source, &staging.join(entry))?;
                continue;
            }
            let inkos_dir = staging.join(".inkos");
            std::fs::create_dir_all(&inkos_dir)?;
            for child in std::fs::read_dir(&source)? {
                let child = child?;
                let name = child.file_name().to_string_lossy().to_string();
                if name == "secrets.json" && !include_secrets {
                    continue;
                }
                copy_recursive(&child.path(), &inkos_dir.join(&name))?;
            }
        }
        // 940 号：includeSessions=1 真正打包 sessions/（原形态参数仅进
        // manifest scope、打包范围静默忽略=参数骗人；sessions/ 为会话
        // JSONL 用户创作上下文）。默认仍排除。
        if include_sessions {
            let sessions_source = root.join("sessions");
            if is_existing(&sessions_source) {
                copy_recursive(&sessions_source, &staging.join("sessions"))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = copy_result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }

    let mut files = Vec::new();
    if list_archive_files(&staging, "", &mut files).is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(std::io::Error::other("staging walk failed"));
    }
    let mut file_count = 0u64;
    let mut total_bytes = 0u64;
    for file in &files {
        file_count += 1;
        total_bytes += std::fs::metadata(staging.join(file)).map(|m| m.len()).unwrap_or(0);
    }
    let manifest = json!({
        "version": 1,
        "createdAt": iso_now(),
        "generator": "inkos-desktop",
        "scope": { "includeSecrets": include_secrets, "includeSessions": include_sessions },
        "fileCount": file_count,
        "totalBytes": total_bytes,
    });
    // 779 号：清单原子写（sync 形态）——撕裂清单=整包备份不可解读。
    if crate::utils::atomic_file_set::write_file_atomic_sync(
        &staging.join(MANIFEST_NAME),
        &serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
    )
    .is_err()
    {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(std::io::Error::other("manifest write failed"));
    }
    // TS buildTarArchive 在 manifest 落盘后二次遍历——manifest 本身必须入包。
    files.clear();
    if list_archive_files(&staging, "", &mut files).is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(std::io::Error::other("staging rewalk failed"));
    }

    let tar_buffer = {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut result = Ok(());
        for file in &files {
            let path = staging.join(file);
            let mut header = tar::Header::new_gnu();
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    result = Err(error);
                    break;
                }
            };
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            let archive_name = format!("{PACKAGE_ROOT_NAME}/{file}");
            if let Err(error) = builder.append_data(&mut header, archive_name, bytes.as_slice()) {
                result = Err(std::io::Error::other(error.to_string()));
                break;
            }
        }
        if result.is_ok() {
            result = builder.finish().map_err(|e| std::io::Error::other(e.to_string()));
        }
        match result {
            Ok(()) => builder.into_inner().and_then(|e| e.finish()).map_err(|e| std::io::Error::other(e.to_string())),
            Err(error) => Err(error),
        }
    };
    let _ = std::fs::remove_dir_all(&staging);
    tar_buffer
}

/// `POST /api/v1/backup/import`——两段式（预览 → `?confirm=1` 写回+快照）。
pub async fn import(
    State(runtime): State<BooksRuntime>,
    Query(params): Query<HashMap<String, String>>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let root = runtime.state.project_root().to_path_buf();
    let confirm = params.get("confirm").map(|v| v == "1").unwrap_or(false);
    // 763 号：同 export——整包解析 + 逐条目落盘移出 runtime 线程。
    let result = tokio::task::spawn_blocking(move || run_import(&root, &body, confirm)).await;
    match result {
        Ok(Ok(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

/// 955 号：当前 Unix 毫秒（时钟早于纪元回退 0，与 954 前快照命名同语义）。
fn current_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn run_import(root: &Path, body: &[u8], confirm: bool) -> Result<serde_json::Value, String> {
    run_import_with_clock(root, body, confirm, IntroducedMode::Ignore, current_millis)
}

/// 956 号：introduced 清单处理模式。import 端点恒 `Ignore`（外部包恢复永不
/// 透传不删除——契约面不变）；rollback 端点 `Keep`（保守回滚：预览透传清单，
/// 确认不删）或 `Delete`（完全回滚 deleteIntroduced=1：预览与确认同一先验
/// ——预览预言确认结果，确认写回后删除）。TS `IntroducedMode` 同语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IntroducedMode {
    Ignore,
    Keep,
    Delete,
}

/// 956 号：快照 manifest 中 introduced 清单的可信度三分（回滚删除面先验）。
/// `Absent` = 无该字段或 kind 非 pre-restore-snapshot（export 包/955- 旧快
/// 照）；`Malformed` = 字段存在但非纯 string 数组（清单不可信）；`Present`
/// = 可用清单。preview 仅 `Present` 且全路径安全才透传；confirm 带删除时
/// `Absent`→lacks、`Malformed`→unsafe（写回前先验，写回不可回退）。
enum IntroducedList {
    Absent,
    Malformed,
    Present(Vec<String>),
}

/// 956 号：读快照 manifest 的 introduced 清单（kind 限定 pre-restore-snapshot
/// ——introduced 语义只对回滚保险快照成立，export 包即便被手工塞入该字段也
/// 不透传不执行）。TS `manifestIntroducedList` 同语义。
fn manifest_introduced_list(manifest: &serde_json::Value) -> IntroducedList {
    if manifest.get("kind").and_then(serde_json::Value::as_str) != Some("pre-restore-snapshot") {
        return IntroducedList::Absent;
    }
    let Some(items) = manifest.get("introduced").and_then(serde_json::Value::as_array) else {
        return IntroducedList::Absent;
    };
    let mut paths = Vec::with_capacity(items.len());
    for item in items {
        let Some(path) = item.as_str() else {
            return IntroducedList::Malformed;
        };
        paths.push(path.to_string());
    }
    IntroducedList::Present(paths)
}

/// 956 号：回滚删除面路径安全判定（TS `isSafeRelativePath` 同判据，纯串级
/// 跨平台一致；向量表 `backup-introduced-path-vectors.json` 双端断言同文件）。
/// introduced 清单来自 manifest JSON 自由字符串，不经 tar 条目安全三则——
/// 不安全路径若放行可删 root 外任意文件（unlink 对绝对/穿越名直达盘外）。
fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() || path.contains('\0') || path.contains('\\') || path.starts_with('/') {
        return false;
    }
    let bytes = path.as_bytes();
    // 盘符形态（`C:/x` 绝对与 `C:x` 驱动器相对）一并拒绝。
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return false;
    }
    // 空段（`a//b`）、当前段（`.`）、穿越段（`..`）一律拒绝——保守整拒不归一化。
    !path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

/// import 两段式执行体（import 与 rollback 共用）。955 号：`now` 为时钟注入
/// 缝（测试固定毫秒以确定性复现同毫秒碰撞），且在恢复串行锁内读取——快照
/// 名取持锁时刻，排队等锁的恢复不会以更早的请求时刻命名更晚的快照。
/// 956 号：`mode` 见 [`IntroducedMode`]——introduced 透传（preview）与删除
/// 语义（confirm+deleteIntroduced=1）只对回滚端点成立。
fn run_import_with_clock(
    root: &Path,
    body: &[u8],
    confirm: bool,
    mode: IntroducedMode,
    now: fn() -> u128,
) -> Result<serde_json::Value, String> {
    let (entries, rejected) = parse_archive(body)?;
    // 安全一则：不安全条目整包拒绝（TS 逐字：`unsafe entries rejected: ...`）。
    if !rejected.is_empty() {
        return Err(format!("unsafe entries rejected: {}", rejected.join(", ")));
    }
    // manifest 查找：TS 导出以 `inkos-backup/` 为包根前缀，导入却按裸名精确
    // 查——导出→导入端到端从未可能成功（R18 产后即搁浅的另一处实证）。
    // 此处按「能工作的契约」实现：前缀名优先，裸名兜底（443 号）。
    let manifest_entry = entries
        .iter()
        .find(|entry| entry.name == format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}"))
        .or_else(|| entries.iter().find(|entry| entry.name == MANIFEST_NAME))
        .ok_or_else(|| "backup-manifest.json missing".to_string())?;
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_entry.bytes)
        .map_err(|_| "backup-manifest.json unreadable".to_string())?;
    let version_ok = manifest.get("version").and_then(serde_json::Value::as_i64) == Some(1);
    let generator_ok = manifest
        .get("generator")
        .and_then(serde_json::Value::as_str)
        .map(|g| g.starts_with("inkos-desktop"))
        .unwrap_or(false);
    if !version_ok || !generator_ok {
        return Err("unsupported backup generator/version".to_string());
    }

    // 恢复目标路径：剥去包根前缀（导出以 inkos-backup/ 打包；恢复必须落到
    // 项目根相对路径——TS 按归档名原样写回，恢复永远落不进真实书目，同为
    // 产后搁浅断点；此处按「能工作的契约」实现）。
    let package_prefix = format!("{PACKAGE_ROOT_NAME}/");
    let restore_name = |name: &str| -> String {
        name.strip_prefix(package_prefix.as_str()).unwrap_or(name).to_string()
    };
    // 955 号：确认段持 per-root 恢复串行锁——覆盖面盘点须在锁内（锁外盘点
    // 与写回之间他者恢复新建的文件会漏出快照=回滚面缺口）；预览只读不持锁。
    let lock = confirm.then(|| restore_lock(root));
    let _guard = lock
        .as_ref()
        .map(|lock| lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    // 将覆盖项/恢复新增项 = root 下同名文件的存在性。955 号：manifest 条目
    // 不参与——写回本就跳过它（根目录同名文件不会被覆盖），954 前却计入覆盖
    // 面：根目录存在 backup-manifest.json 时预览误报「将覆盖」，且该文件以裸
    // 名入快照、与快照自身 manifest 同名并存——回滚时裸名查找先命中用户文件，
    // 先验失败=快照不可回滚。956 号：单次遍历双清单（覆盖面=存在，introduced
    // =不存在）——同一时刻同一存在性视图，撤销恢复=写回 overwriting+可选删
    // 除 introduced=完全回到快照时刻（952 备案「部分回滚边界」清偿）。
    let mut overwriting: Vec<String> = Vec::new();
    let mut introduced: Vec<String> = Vec::new();
    for entry in entries.iter().filter(|entry| !is_manifest_entry(&entry.name)) {
        let name = restore_name(&entry.name);
        if is_existing(&root.join(&name)) {
            overwriting.push(name);
        } else {
            introduced.push(name);
        }
    }

    // 956 号：完全回滚删除面先验——必须在任何盘面变更前（写回一旦发生不可
    // 回退，先验失败须整体 400 无副作用）。仅 `Delete` 模式进入，预览与确认
    // 同一先验（预览预言确认结果）；`Absent`（export 包/955- 旧快照）、
    // `Malformed`（清单非纯 string 数组）、含不安全路径的 `Present` 一律拒绝
    // ——不安全路径若放行可删 root 外任意文件。重复项去重（篡改清单的重复名
    // 不得重复留档/重复计数）。
    let delete_list: Option<Vec<String>> = if mode == IntroducedMode::Delete {
        match manifest_introduced_list(&manifest) {
            IntroducedList::Absent => {
                return Err("snapshot manifest lacks introduced list".to_string());
            }
            IntroducedList::Malformed => return Err("unsafe introduced path".to_string()),
            IntroducedList::Present(paths) => {
                if let Some(bad) = paths.iter().find(|path| !is_safe_relative_path(path)) {
                    return Err(format!("unsafe introduced path: {bad}"));
                }
                let mut seen = std::collections::HashSet::with_capacity(paths.len());
                Some(paths.into_iter().filter(|path| seen.insert(path.clone())).collect())
            }
        }
    } else {
        None
    };

    if !confirm {
        // 955 号：计数面=数据条目（manifest 元数据除外）=写回面——954 前把
        // manifest 计入 fileCount/newFiles：真实导出包预览「新增 1」而实际不
        // 写任何新文件，且预览总数比确认后的 restored 多 1。
        let file_count = entries.iter().filter(|entry| !is_manifest_entry(&entry.name)).count();
        let new_files = file_count - overwriting.len();
        // 956 号：回滚预览透传 introduced 清单（快照记录的恢复期间新增文件，
        // 供 UI 呈现「可一并删除」决策面）。仅回滚端点且清单可用且全路径安全
        // 才透传——不安全路径绝不入 UI（用户看不到删除区就不会选择删除；即便
        // 绕过 UI 强发 confirm，Delete 模式先验仍 400）。
        let mut preview = json!({
            "fileCount": file_count,
            "overwriting": overwriting,
            "newFiles": new_files,
        });
        if mode != IntroducedMode::Ignore {
            if let IntroducedList::Present(paths) = manifest_introduced_list(&manifest) {
                if paths.iter().all(|path| is_safe_relative_path(path)) {
                    // 键序双端无需字节一致（serde_json 默认 BTreeMap 排序 vs TS
                    // 构造序；差分器 parse 后结构对比，消费方按字段名取值）。
                    preview["introduced"] = json!(paths);
                }
            }
        }
        return Ok(json!({ "preview": preview }));
    }

    // 956 号：完全回滚时待删文件须先留档（每个破坏性操作都有保险，951 号不
    // 变式——删除前内容入本次快照，回滚该快照即复原=完全回滚自身也有保险）。
    // 仅常规文件可删（symlink_metadata 不跟随，与 955 回滚面同判据：目录/
    // 链接/特殊文件不删且报 deleteFailed）；已不存在=终态一致直接跳过；其余
    // 读取失败不留档即不删，同样报 deleteFailed（用户要求删除而未删，不得静默）。
    let mut deletable: Vec<String> = Vec::new();
    let mut deletable_files: Vec<Vec<u8>> = Vec::new();
    let mut delete_failed: Vec<serde_json::Value> = Vec::new();
    for name in delete_list.iter().flatten() {
        let target = root.join(name);
        match std::fs::symlink_metadata(&target) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                delete_failed.push(json!({ "path": name, "error": e.to_string() }));
                continue;
            }
            Ok(meta) if !meta.is_file() => {
                delete_failed.push(json!({ "path": name, "error": "not a regular file" }));
                continue;
            }
            Ok(_) => {}
        }
        match std::fs::read(&target) {
            Ok(data) => {
                deletable.push(name.clone());
                deletable_files.push(data);
            }
            Err(e) => delete_failed.push(json!({ "path": name, "error": e.to_string() })),
        }
    }

    // 恢复前自动快照（回滚保险）。956 号：本次操作改变盘面即留快照——覆盖/
    // 新建/删除任一非空。此前仅 overwriting 非空才留：纯新增恢复无任何回滚
    // 保险；且纯新增恢复产的快照（无数据条目）被完全回滚时若不计 deletable，
    // 待删文件将被无保险删除。
    let has_snapshot = !overwriting.is_empty() || !introduced.is_empty() || !deletable.is_empty();
    if has_snapshot {
        let backups_dir = root.join("backups");
        std::fs::create_dir_all(&backups_dir).map_err(|e| e.to_string())?;
        // 955 号：唯一名分配（持锁探测）——954 前同毫秒二次恢复 rename 覆盖
        // 前一份快照，回滚保险静默丢失。
        let millis = allocate_snapshot_millis(&backups_dir, now())?;
        let snapshot_path = backups_dir.join(format!("pre-restore-{millis}.tar"));
        // 782 号：快照是恢复的回滚保险件——「恢复中掉电」窗口内撕裂 tar 等于
        // 保险失效（恢复目标本尊已由 779 号 write_file_atomic_sync 兜住，此处
        // 补齐对称面）。tar::Builder 直拥句柄，走 tmp+sync_all+rename 形态
        // （对齐 llm/secrets save_secrets 惯用形）；失败清理 tmp 防残留。
        let tmp_path = backups_dir.join(format!(".pre-restore-{millis}.tmp"));
        let build = || -> Result<(), String> {
            let file = std::fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
            let mut builder = tar::Builder::new(file);
            for name in &overwriting {
                if let Ok(data) = std::fs::read(root.join(name)) {
                    let mut header = tar::Header::new_gnu();
                    header.set_size(data.len() as u64);
                    header.set_mode(0o644);
                    header.set_cksum();
                    builder
                        .append_data(&mut header, name, data.as_slice())
                        .map_err(|e| e.to_string())?;
                }
            }
            // 956 号：完全回滚（Delete 模式）时待删文件将真实删除——删除前
            // 先入快照（内容=删除前时刻；回滚本保险快照即可恢复它们=完全回
            // 滚自身也有保险）。条目在 overwriting 之后。
            for (name, data) in deletable.iter().zip(deletable_files.iter()) {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, name, data.as_slice())
                    .map_err(|e| e.to_string())?;
            }
            // 951 号：快照附带 manifest（裸名形态，双端 import 端点均兼容
            // 裸名查找与写回跳过）——pre-restore 快照可直接经
            // backup/import?confirm=1 回灌=回滚保险从「留档」到「可执行」
            // 闭环。956 号：manifest 补撤销决策面（三数组恒在，形状统一）
            // ——撤销本次操作=写回快照数据条目（overwriting 与 removing 的
            // 删除前内容）+ 可选删除 introduced（本次操作新建的文件）=精确
            // 逆运算。removing 仅 Delete 模式非空（本次将删的已留档文件）。
            let snapshot_manifest = serde_json::json!({
                "version": 1,
                "generator": "inkos-desktop",
                "kind": "pre-restore-snapshot",
                "overwriting": overwriting,
                "introduced": introduced,
                "removing": deletable,
            })
            .to_string()
            .into_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_size(snapshot_manifest.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, MANIFEST_NAME, snapshot_manifest.as_slice())
                .map_err(|e| e.to_string())?;
            let file = builder.into_inner().map_err(|e| e.to_string())?;
            // 762 号同款：落盘后才 rename，掉电后快照要么缺失要么完整。
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            std::fs::rename(&tmp_path, &snapshot_path).map_err(|e| e.to_string())?;
            Ok(())
        };
        if let Err(error) = build() {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(error);
        }
    }

    // 写回（快照已兜底）。manifest 是元数据不落盘；restored 计实际写回文件数。
    let mut restored = 0usize;
    for entry in &entries {
        if is_manifest_entry(&entry.name) {
            continue;
        }
        let target = root.join(restore_name(&entry.name));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // 779 号：恢复是覆盖写——撕裂=毁掉现有文件换成残缺恢复件，原子替换。
        crate::utils::atomic_file_set::write_file_atomic_sync(&target, &entry.bytes)
            .map_err(|e| e.to_string())?;
        restored += 1;
    }
    // 956 号：完全回滚删除执行（写回全部完成后）——名单=已留档的 deletable
    // （不留档不删）。留档与删除之间被外部删除（ENOENT）=终态一致计入；其他
    // 失败收集 deleteFailed 且保持 200——写回已成功，谎报 500 会诱导用户重试
    // （重复快照+写回）。空目录不删（只删文件——不可能误删用户目录内容）。
    let mut removed = 0usize;
    for path in &deletable {
        match std::fs::remove_file(root.join(path)) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => removed += 1,
            Err(e) => delete_failed.push(json!({ "path": path, "error": e.to_string() })),
        }
    }
    let mut response = json!({ "ok": true, "restored": restored, "snapshot": has_snapshot });
    if delete_list.is_some() {
        response["removed"] = json!(removed);
        if !delete_failed.is_empty() {
            response["deleteFailed"] = json!(delete_failed);
        }
    }
    Ok(response)
}

// ── 954 号：快照回滚用户可执行闭环——列举 + 服务端回滚执行 ──
//
// 951 的 pre-restore 快照此前只留档（回滚须手工到文件系统取 tar 灌回
// import）；本轮补齐：`GET /api/v1/backup/snapshots` 列举 +
// `POST /api/v1/backup/rollback` 服务端读快照字节走 run_import 同一管道
// （安全三则+manifest 先验+两段式 preview/confirm 同构——快照回滚过与
// 外部备份包完全相同的检查，非语义旁路；回滚执行时对被覆盖文件再留快照
// =import 不变式自动保持）。

/// 955 号：快照名规范形态（`pre-restore-<D>.tar`，D = `0` 或无前导零的
/// 1-16 位 ASCII 十进制数字）——回滚面唯一放行形态，路径穿越无从谈起。
/// 双端纯串级判定逐字对齐（TS `isSnapshotName`；向量表
/// `packages/studio/src/__tests__/golden/backup-snapshot-name-vectors.json`
/// 双端断言同文件）。954 版按数值解析判定（TS 安全整数 vs 此处 u64）在
/// 16-19 位名上双端分叉（TS 判非法名、此处判缺失且列举可见），本形态不依
/// 赖任何数值语义；规范形态=名与毫秒一一对应。
fn is_snapshot_name(name: &str) -> bool {
    let Some(stem) = name
        .strip_prefix("pre-restore-")
        .and_then(|rest| rest.strip_suffix(".tar"))
    else {
        return false;
    };
    matches!(stem.len(), 1..=16)
        && stem.bytes().all(|b| b.is_ascii_digit())
        && (stem == "0" || !stem.starts_with('0'))
}

/// 955 号：快照名数值序比较——规范形态同前后缀，(长度, 字典序) 即毫秒数值
/// 序（954 版纯字典序在位数不同的名上错序：`pre-restore-9` 排在 13 位新快
/// 照之前）。TS `compareSnapshotNames` 同算法。
fn compare_snapshot_names(a: &str, b: &str) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// 955 号：快照名探测上限（同毫秒连续占用的病态防护；正常 1 次命中）。
const SNAPSHOT_NAME_PROBE_LIMIT: u128 = 1000;

/// 955 号：快照唯一名分配——自 `now_millis` 起探测首个未占用毫秒名（任意
/// 类型占用均跳过：同名目录也会令 rename 失败）。调用方须持 per-root 恢复
/// 串行锁（探测→就位之间无进程内竞争者）；非 NotFound 的探测错误直接上抛，
/// 不把不可访问误判为占用而空转。
fn allocate_snapshot_millis(backups_dir: &Path, now_millis: u128) -> Result<u128, String> {
    for millis in now_millis..now_millis.saturating_add(SNAPSHOT_NAME_PROBE_LIMIT) {
        match std::fs::symlink_metadata(backups_dir.join(format!("pre-restore-{millis}.tar"))) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(millis),
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
    }
    Err("no free snapshot name".to_string())
}

/// 955 号：per-root 恢复串行锁——确认段（覆盖面盘点→快照→写回）整体互斥
/// （954 前并发两次恢复：写回交错成混合态，同毫秒共写
/// `.pre-restore-<ms>.tmp` 撕裂快照）。run_import 只在 spawn_blocking 线程
/// 执行，std Mutex 阻塞合法；锁内数据为 ()，中毒无不变式可破——into_inner
/// 恢复（某次恢复 panic 不永久锁死备份域）。条目按项目根常驻（task_store
/// path_lock 同款判定：根数有限无泄漏）。
fn restore_lock(root: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.entry(root.to_path_buf()).or_default().clone()
}

/// 列举 `backups/pre-restore-*.tar`（规范形态+常规文件过滤，数值倒序=时间
/// 倒序）。目录缺失=空列表；tmp 残留 `.pre-restore-*.tmp`、同名目录、符号
/// 链接天然被过滤（DirEntry::metadata 不跟随链接）。
fn list_snapshots(root: &Path) -> serde_json::Value {
    let mut items: Vec<(String, u64)> = Vec::new();
    if let Ok(children) = std::fs::read_dir(root.join("backups")) {
        for child in children.flatten() {
            let Ok(meta) = child.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = child.file_name().to_string_lossy().to_string();
            if !is_snapshot_name(&name) {
                continue;
            }
            items.push((name, meta.len()));
        }
    }
    items.sort_by(|a, b| compare_snapshot_names(&b.0, &a.0));
    json!({
        "snapshots": items.iter().map(|(name, size)| json!({ "name": name, "size": size })).collect::<Vec<_>>()
    })
}

#[derive(Debug)]
enum RollbackFailure {
    InvalidName,
    NotFound,
    Import(String),
}

/// 快照回滚：校验形态 → 读快照字节 → 走 import 同一执行体。955 号：回滚面=
/// 列举面——仅放行常规文件（符号链接/目录按缺失处理，与 list_snapshots 同
/// 判据：954 版 fs::read 跟随链接，列举不可见的链接名可被直接回滚）。
/// 956 号：`delete_introduced` 透传删除语义（rollback 是唯一放行端点）。
fn run_rollback(
    root: &Path,
    name: &str,
    confirm: bool,
    delete_introduced: bool,
) -> Result<serde_json::Value, RollbackFailure> {
    if !is_snapshot_name(name) {
        return Err(RollbackFailure::InvalidName);
    }
    let path = root.join("backups").join(name);
    let is_regular = std::fs::symlink_metadata(&path).map(|meta| meta.is_file()).unwrap_or(false);
    if !is_regular {
        return Err(RollbackFailure::NotFound);
    }
    let bytes = std::fs::read(&path).map_err(|_| RollbackFailure::NotFound)?;
    let mode = if delete_introduced { IntroducedMode::Delete } else { IntroducedMode::Keep };
    run_import_with_clock(root, &bytes, confirm, mode, current_millis).map_err(RollbackFailure::Import)
}

/// `GET /api/v1/backup/snapshots`——列举可回滚快照。
pub async fn snapshots(State(runtime): State<BooksRuntime>) -> impl IntoResponse {
    let root = runtime.state.project_root().to_path_buf();
    // 763 号同款：盘面列举移出 runtime 线程。955 号：JoinError 走 500（与同
    // 文件 import/rollback 一致——954 版吞成 200 空列表会把 panic 伪装成
    // 「无快照」）。
    match tokio::task::spawn_blocking(move || list_snapshots(&root)).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

/// `POST /api/v1/backup/rollback[?confirm=1][&deleteIntroduced=1]`——回滚指
/// 定快照（name/confirm 走 query，同族语义；线格式与 import 两段式同构）。
/// 956 号：deleteIntroduced=1=完全回滚（预览透传 introduced，确认在写回后
/// 一并删除快照记录的恢复期间新增文件——每份待删文件删除前留档）。
pub async fn rollback(
    State(runtime): State<BooksRuntime>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let root = runtime.state.project_root().to_path_buf();
    let confirm = params.get("confirm").map(|v| v == "1").unwrap_or(false);
    let delete_introduced = params.get("deleteIntroduced").map(|v| v == "1").unwrap_or(false);
    let name = params.get("name").cloned().unwrap_or_default();
    let result =
        tokio::task::spawn_blocking(move || run_rollback(&root, &name, confirm, delete_introduced))
            .await;
    match result {
        Ok(Ok(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(Err(RollbackFailure::InvalidName)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid snapshot name" })),
        )
            .into_response(),
        Ok(Err(RollbackFailure::NotFound)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "snapshot not found" })),
        )
            .into_response(),
        Ok(Err(RollbackFailure::Import(error))) => {
            (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

/// 解析（可能 gzip 的）tar 归档；安全三则内置（TS `tar-read.ts` 同语义）。
/// 返回 `(文件条目, 被拒绝条目名)`——拒绝项非空时调用方整包 400。
fn parse_archive(body: &[u8]) -> Result<(Vec<ArchiveEntry>, Vec<String>), String> {
    let mut entries = Vec::new();
    let mut rejected = Vec::new();
    let mut total: u64 = 0;
    let is_gzip = body.len() >= 2 && body[0] == 0x1f && body[1] == 0x8b;
    fn collect_entry<R: std::io::Read>(
        mut entry: tar::Entry<'_, R>,
        entries: &mut Vec<ArchiveEntry>,
        rejected: &mut Vec<String>,
        total: &mut u64,
    ) -> Result<(), String> {
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();
        let name = path.to_string_lossy().replace('\\', "/");
        // 安全一则：`..` 与绝对路径拒绝（TS 逐字 substring 语义）。
        if name.contains("..") || name.starts_with('/') {
            rejected.push(name);
            return Ok(());
        }
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            return Ok(()); // 目录条目不产出文件
        }
        if !entry_type.is_file() {
            // 安全二则：仅普通文件（链接/特殊类型拒绝）。
            rejected.push(name);
            return Ok(());
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        // 安全三则：总解压字节上限（防炸弹）。
        *total += bytes.len() as u64;
        if *total > TAR_MAX_TOTAL_BYTES {
            return Err(format!("tar exceeds max total bytes ({TAR_MAX_TOTAL_BYTES})"));
        }
        entries.push(ArchiveEntry { name, bytes });
        Ok(())
    }
    let read_result = (|| -> Result<(), String> {
        if is_gzip {
            for entry in tar::Archive::new(GzDecoder::new(body))
                .entries()
                .map_err(|e| e.to_string())?
            {
                collect_entry(entry.map_err(|e| e.to_string())?, &mut entries, &mut rejected, &mut total)?;
            }
        } else {
            for entry in tar::Archive::new(body).entries().map_err(|e| e.to_string())? {
                collect_entry(entry.map_err(|e| e.to_string())?, &mut entries, &mut rejected, &mut total)?;
            }
        }
        Ok(())
    })();
    read_result?;
    Ok((entries, rejected))
}

/// ISO-8601 UTC 时间戳（manifest createdAt；避免为单一字段引入 chrono）。
fn iso_now() -> String {
    let millis = current_millis();
    let secs = (millis / 1000) as u64;
    let ms = millis % 1000;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// 天数 → (y, m, d)（Howard Hinnant civil_from_days 算法，公开领域）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("books/b1/story")).unwrap();
        std::fs::write(
            root.join("books/b1/book.json"),
            r#"{"id":"b1","title":"t","platform":"other","genre":"other","status":"active"}"#,
        )
        .unwrap();
        std::fs::write(root.join("books/b1/story/current_state.md"), "# 当前状态\n\n- 章节一。\n").unwrap();
        std::fs::write(root.join("inkos.json"), r#"{"name":"walkthrough"}"#).unwrap();
        std::fs::create_dir_all(root.join(".inkos")).unwrap();
        std::fs::write(root.join(".inkos/secrets.json"), r#"{"services":{}}"#).unwrap();
        std::fs::create_dir_all(root.join("prompt")).unwrap();
        std::fs::write(root.join("prompt/writer.md"), "写作守则。").unwrap();
        // 938/939 号：works+五新目录种子（用户创作数据，备份域补缺的断言对象）。
        // 949 号：manifest 补齐 serde 必填面（version/lineage/metadata/时间戳）
        // + source/ 目录——种子成为合法 work，打包前 sync 全走成功路径。
        std::fs::create_dir_all(root.join("works/w1/source")).unwrap();
        std::fs::write(
            root.join("works/w1/work.json"),
            r#"{"version":2,"id":"w1","title":"坊市旧事","profileId":"short-story","language":"zh","status":"draft","lineage":[],"artifacts":[],"metadata":{},"createdAt":"2026-11-09T00:00:00.000Z","updatedAt":"2026-11-09T00:00:00.000Z"}"#,
        )
        .unwrap();
        for dir in ["translations", "genres", "skills", "radar", "worlds"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
            std::fs::write(root.join(format!("{dir}/data.json")), format!("{{\"seeded\":\"{dir}\"}}")).unwrap();
        }
        (dir, root)
    }

    /// TS `createTarHeader` 对偶的最小 tar 构建（name+payload+双零块），供导入用例构造。
    fn simple_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, payload) in entries {
            let mut header = vec![0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}", payload.len());
            header[124..124 + size.len()].copy_from_slice(size.as_bytes());
            header[156] = b'0';
            // 合法校验和（tar crate 解析强制；TS 自研读取器不校验所以此前缺席）。
            for byte in &mut header[148..156] {
                *byte = b' ';
            }
            let sum: u32 = header.iter().map(|&b| b as u32).sum();
            let octal = format!("{:06o}\0 ", sum);
            header[148..156].copy_from_slice(octal.as_bytes());
            out.extend_from_slice(&header);
            out.extend_from_slice(payload);
            let padding = (512 - (payload.len() % 512)) % 512;
            out.extend(std::iter::repeat_n(0u8, padding));
        }
        out.extend(std::iter::repeat_n(0u8, 1024));
        out
    }

    fn manifest_bytes(generator: &str, version: i64) -> Vec<u8> {
        serde_json::json!({ "version": version, "generator": generator }).to_string().into_bytes()
    }

    /// 949 号：打包前 sync 循环单元面——手工放置 source 补登为 candidate
    /// 修订（accept=false 不翻转 accept 面）、杂散文件跳过、坏 work 隔离。
    #[tokio::test]
    async fn backup_sync_covers_manual_works_and_isolates_failures() {
        let (_dir, root) = temp_root();
        // 手工放置形态：source 有文件、manifest 未注册（947 备案的补齐对象）。
        std::fs::write(root.join("works/w1/source/outline.md"), "# 大纲\n\n- 第一幕。\n").unwrap();
        // Finder .DS_Store 形态杂散文件 + 坏 work 目录——不得影响 w1 补齐。
        std::fs::write(root.join("works/.DS_Store"), "junk").unwrap();
        std::fs::create_dir_all(root.join("works/w-broken")).unwrap();
        std::fs::write(root.join("works/w-broken/work.json"), "{not-json").unwrap();
        sync_works_for_backup(&root).await;
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("works/w1/work.json")).unwrap(),
        )
        .unwrap();
        let artifacts = manifest["artifacts"].as_array().unwrap();
        assert_eq!(artifacts.len(), 1, "{manifest}");
        assert_eq!(artifacts[0]["currentRevisionId"], serde_json::Value::Null, "{manifest}");
        assert_eq!(artifacts[0]["revisions"][0]["status"], "candidate", "{manifest}");
        let snapshot = artifacts[0]["revisions"][0]["snapshotPath"].as_str().unwrap();
        assert!(root.join("works/w1").join(snapshot).is_file(), "{snapshot}");
    }

    #[test]
    fn export_roundtrip_contains_scope_and_excludes_secrets() {
        let (_dir, root) = temp_root();
        let gz = run_export(&root, false, false).unwrap();
        let entries = parse_archive(&gz).unwrap().0;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.iter().all(|n| n.starts_with("inkos-backup/")), "{names:?}");
        assert!(names.contains(&"inkos-backup/backup-manifest.json"), "{names:?}");
        assert!(names.contains(&"inkos-backup/books/b1/book.json"), "{names:?}");
        assert!(names.contains(&"inkos-backup/prompt/writer.md"), "{names:?}");
        assert!(names.contains(&"inkos-backup/works/w1/work.json"), "{names:?}"); // 938 号：works 域入备份
        // 939 号：五新目录种子条目全部入包。
        for dir in ["translations", "genres", "skills", "radar", "worlds"] {
            assert!(
                names.contains(&format!("inkos-backup/{dir}/data.json").as_str()),
                "{names:?}"
            );
        }
        assert!(!names.iter().any(|n| n.contains("secrets.json")), "secrets must be excluded: {names:?}");
        let manifest: serde_json::Value = serde_json::from_slice(
            entries
                .iter()
                .find(|e| e.name.ends_with(MANIFEST_NAME))
                .unwrap()
                .bytes
                .as_slice(),
        )
        .unwrap();
        assert_eq!(manifest["generator"], "inkos-desktop");
        assert_eq!(manifest["scope"]["includeSecrets"], false);
        assert_eq!(manifest["fileCount"], (names.len() - 1) as u64); // manifest 自身不计
    }

    #[test]
    fn export_include_secrets_keeps_secrets() {
        let (_dir, root) = temp_root();
        let gz = run_export(&root, true, false).unwrap();
        let entries = parse_archive(&gz).unwrap().0;
        assert!(entries.iter().any(|e| e.name.contains("secrets.json")));
    }

    #[test]
    fn import_preview_reports_overwriting_and_new_files() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"id":"b1"}"#),
            ("inkos-backup/books/b2/new.md", b"new file"),
        ]);
        let response = run_import(&root, &tar_bytes, false).unwrap();
        let preview = response.get("preview").unwrap();
        // 955 号：计数面=数据条目（manifest 元数据不计）——954 前 3/2 口径把
        // manifest 报成「新增文件」，预览总数比确认 restored 多 1。
        assert_eq!(preview["fileCount"], 2);
        assert_eq!(preview["overwriting"], serde_json::json!(["books/b1/book.json"]));
        assert_eq!(preview["newFiles"], 1);
        // 预览总数=确认后的写回数（用户看到的两个数字一致）。
        assert_eq!(run_import(&root, &tar_bytes, true).unwrap()["restored"], 2);
    }

    #[test]
    fn import_confirm_writes_back_and_snapshots_overwritten() {
        let (dir, root) = temp_root();
        let original = std::fs::read(root.join("books/b1/book.json")).unwrap();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"id":"b1","restored":true}"#),
        ]);
        let response = run_import(&root, &tar_bytes, true).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["restored"], 1); // manifest 不落盘
        assert_eq!(response["snapshot"], true);
        // 写回生效
        let restored = std::fs::read(root.join("books/b1/book.json")).unwrap();
        assert!(String::from_utf8_lossy(&restored).contains("restored"));
        // 快照含原文件内容（回滚保险）
        let backups = std::fs::read_dir(root.join("backups")).unwrap().count();
        assert_eq!(backups, 1);
        let snapshot_path = root
            .join("backups")
            .join(std::fs::read_dir(root.join("backups")).unwrap().next().unwrap().unwrap().file_name());
        let snapshot_entries = parse_archive(&std::fs::read(&snapshot_path).unwrap()).unwrap().0;
        assert_eq!(snapshot_entries.len(), 2); // 951 号起：快照附 manifest 条目
        assert_eq!(snapshot_entries[0].name, "books/b1/book.json");
        assert_eq!(snapshot_entries[0].bytes, original);
        // 951 号：回滚闭环——快照（附 manifest）直接灌回 import，回到导入前状态。
        let snapshot_bytes = std::fs::read(&snapshot_path).unwrap();
        let rollback = run_import(&root, &snapshot_bytes, true).unwrap();
        assert_eq!(rollback["ok"], true);
        assert_eq!(rollback["restored"], 1); // manifest 不计
        let rolled_back = std::fs::read(root.join("books/b1/book.json")).unwrap();
        assert_eq!(rolled_back, original);
        drop(dir);
    }

    // ── 954 号：快照回滚用户可执行闭环（列举 + 服务端回滚执行）──

    #[test]
    fn snapshots_list_filters_and_sorts_pre_restore_tars() {
        let (_dir, root) = temp_root();
        // 缺 backups/ 目录 = 空列表（不报错）。
        assert_eq!(list_snapshots(&root)["snapshots"], serde_json::json!([]));
        std::fs::create_dir_all(root.join("backups")).unwrap();
        std::fs::write(root.join("backups/pre-restore-1731100000000.tar"), b"old").unwrap();
        std::fs::write(root.join("backups/pre-restore-1731200000000.tar"), b"newer-longer").unwrap();
        // 形态外残留：tmp 半成品、杂散文件、同名目录——一律不入列。
        std::fs::write(root.join("backups/.pre-restore-1731300000000.tmp"), b"partial").unwrap();
        std::fs::write(root.join("backups/other.tar"), b"junk").unwrap();
        std::fs::create_dir_all(root.join("backups/pre-restore-1731400000000.tar")).unwrap();
        let items = list_snapshots(&root)["snapshots"].as_array().unwrap().clone();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["name"], "pre-restore-1731200000000.tar"); // 名倒序=时间倒序
        assert_eq!(items[0]["size"], 12);
        assert_eq!(items[1]["name"], "pre-restore-1731100000000.tar");
        assert_eq!(items[1]["size"], 3);
    }

    #[test]
    fn rollback_rejects_invalid_or_missing_names() {
        let (_dir, root) = temp_root();
        // 形态守卫：穿越/非数字/空名一律 InvalidName（先于盘面访问）。
        assert!(matches!(run_rollback(&root, "../../etc/passwd", false, false), Err(RollbackFailure::InvalidName)));
        assert!(matches!(run_rollback(&root, "pre-restore-abc.tar", false, false), Err(RollbackFailure::InvalidName)));
        assert!(matches!(run_rollback(&root, "", false, false), Err(RollbackFailure::InvalidName)));
        // 形态合法但文件不存在。
        assert!(matches!(
            run_rollback(&root, "pre-restore-1731100000000.tar", false, false),
            Err(RollbackFailure::NotFound)
        ));
    }

    #[test]
    fn rollback_restores_snapshot_via_import_pipeline_two_phase() {
        let (dir, root) = temp_root();
        let original = std::fs::read(root.join("books/b1/book.json")).unwrap();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"id":"b1","tampered":true}"#),
        ]);
        run_import(&root, &tar_bytes, true).unwrap(); // 产生 pre-restore 快照（含 original）
        let snapshot_name = {
            let mut names: Vec<String> = std::fs::read_dir(root.join("backups"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
                .collect();
            names.sort();
            names.pop().unwrap()
        };
        // preview 段：服务端读快照走 import 同一管道（安全三则+manifest 先验生效）。
        let preview = run_rollback(&root, &snapshot_name, false, false).unwrap();
        assert_eq!(preview["preview"]["fileCount"], 1); // 955 号：manifest 不计（数据条目=写回面）
        assert_eq!(preview["preview"]["overwriting"], serde_json::json!(["books/b1/book.json"]));
        // confirm 段：回滚执行——book.json 回 original。
        let done = run_rollback(&root, &snapshot_name, true, false).unwrap();
        assert_eq!(done["ok"], true);
        assert_eq!(done["restored"], 1); // manifest 不计
        assert_eq!(done["snapshot"], true);
        assert_eq!(std::fs::read(root.join("books/b1/book.json")).unwrap(), original);
        // 回滚自身也留快照（import 不变式自动保持）：backups/ 现有 2 份。
        assert_eq!(std::fs::read_dir(root.join("backups")).unwrap().count(), 2);
        drop(dir);
    }

    // ── 955 号：954 新增代码审查修复（命名碰撞/并发串行/形态对齐/回滚面=列举面）──

    /// 双端共享向量表（TS backup-routes.test.ts 断言同文件）。
    const SNAPSHOT_NAME_VECTORS: &str =
        include_str!("../../../packages/studio/src/__tests__/golden/backup-snapshot-name-vectors.json");

    fn listed_snapshot_names(root: &Path) -> Vec<String> {
        list_snapshots(root)["snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["name"].as_str().unwrap().to_string())
            .collect()
    }

    fn snapshot_book(root: &Path, name: &str) -> Vec<u8> {
        let entries = parse_archive(&std::fs::read(root.join("backups").join(name)).unwrap()).unwrap().0;
        entries.into_iter().find(|entry| entry.name == "books/b1/book.json").unwrap().bytes
    }

    fn marker_package(marker: &str) -> Vec<u8> {
        simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", format!("{{\"marker\":\"{marker}\"}}").as_bytes()),
            ("inkos-backup/inkos.json", format!("{{\"name\":\"{marker}\"}}").as_bytes()),
        ])
    }

    #[test]
    fn snapshot_name_shape_matches_shared_golden_vectors() {
        let vectors: serde_json::Value = serde_json::from_str(SNAPSHOT_NAME_VECTORS).unwrap();
        for name in vectors["accept"].as_array().unwrap() {
            let name = name.as_str().unwrap();
            assert!(is_snapshot_name(name), "accept: {name:?}");
        }
        for name in vectors["reject"].as_array().unwrap() {
            let name = name.as_str().unwrap();
            assert!(!is_snapshot_name(name), "reject: {name:?}");
        }
        // 排序向量：落盘为常规文件后经 list_snapshots 列举=数值倒序（954 版纯
        // 字典序把 pre-restore-9 排在 13 位新快照之前）。
        let (_dir, root) = temp_root();
        std::fs::create_dir_all(root.join("backups")).unwrap();
        for name in vectors["sortedDesc"]["input"].as_array().unwrap() {
            std::fs::write(root.join("backups").join(name.as_str().unwrap()), b"x").unwrap();
        }
        let expected: Vec<String> = vectors["sortedDesc"]["expected"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap().to_string())
            .collect();
        assert_eq!(listed_snapshot_names(&root), expected);
    }

    #[test]
    fn snapshot_allocation_skips_occupied_millis() {
        let dir = tempfile::tempdir().unwrap();
        let backups = dir.path();
        assert_eq!(allocate_snapshot_millis(backups, 1_731_100_000_000).unwrap(), 1_731_100_000_000);
        std::fs::write(backups.join("pre-restore-1731100000000.tar"), b"a").unwrap();
        // 同名目录同样视为占用（rename 到目录会失败，不能当空位）。
        std::fs::create_dir_all(backups.join("pre-restore-1731100000001.tar")).unwrap();
        assert_eq!(allocate_snapshot_millis(backups, 1_731_100_000_000).unwrap(), 1_731_100_000_002);
    }

    /// 同毫秒二次恢复：两份快照并存且首份内容不被覆盖（954 前 rename 覆盖=
    /// 回滚保险静默丢失；固定时钟确定性复现）。
    #[test]
    fn same_millis_restores_keep_both_snapshots() {
        let (dir, root) = temp_root();
        let original = std::fs::read(root.join("books/b1/book.json")).unwrap();
        let first = run_import_with_clock(&root, &marker_package("A"), true, IntroducedMode::Ignore, || 1_731_100_000_000).unwrap();
        assert_eq!(first["snapshot"], true);
        let second = run_import_with_clock(&root, &marker_package("B"), true, IntroducedMode::Ignore, || 1_731_100_000_000).unwrap();
        assert_eq!(second["snapshot"], true);
        assert_eq!(
            listed_snapshot_names(&root),
            vec!["pre-restore-1731100000001.tar", "pre-restore-1731100000000.tar"]
        );
        // 首份=首次恢复前（original），次份=二次恢复前（A 的恢复结果）。
        assert_eq!(snapshot_book(&root, "pre-restore-1731100000000.tar"), original);
        assert_eq!(snapshot_book(&root, "pre-restore-1731100000001.tar"), br#"{"marker":"A"}"#.to_vec());
        drop(dir);
    }

    /// 并发两次确认恢复：串行锁保证终态为某一包整体（不交错）、两份快照并存，
    /// 且后执行者的快照恰为先执行者的恢复结果（954 前同毫秒共写同一 tmp/终名）。
    #[test]
    fn concurrent_restores_serialize_with_distinct_snapshots() {
        let (dir, root) = temp_root();
        let original = std::fs::read(root.join("books/b1/book.json")).unwrap();
        let (pack_a, pack_b) = (marker_package("A"), marker_package("B"));
        std::thread::scope(|scope| {
            let a = scope.spawn(|| run_import_with_clock(&root, &pack_a, true, IntroducedMode::Ignore, || 1_731_100_000_000));
            let b = scope.spawn(|| run_import_with_clock(&root, &pack_b, true, IntroducedMode::Ignore, || 1_731_100_000_000));
            assert_eq!(a.join().unwrap().unwrap()["ok"], true);
            assert_eq!(b.join().unwrap().unwrap()["ok"], true);
        });
        let book = std::fs::read_to_string(root.join("books/b1/book.json")).unwrap();
        let config = std::fs::read_to_string(root.join("inkos.json")).unwrap();
        let (winner, loser) = if book.contains("\"A\"") { ("A", "B") } else { ("B", "A") };
        assert!(config.contains(&format!("\"{winner}\"")), "写回交错: {book} / {config}");
        let names = listed_snapshot_names(&root);
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(snapshot_book(&root, &names[1]), original);
        assert_eq!(snapshot_book(&root, &names[0]), format!("{{\"marker\":\"{loser}\"}}").into_bytes());
        drop(dir);
    }

    /// 回滚面=列举面：列举不可见的符号链接名不得被直接回滚（954 版 fs::read
    /// 跟随链接）；真快照本体照常可回滚（守卫不误伤）。
    #[cfg(unix)]
    #[test]
    fn rollback_rejects_symlinked_snapshot_names() {
        let (dir, root) = temp_root();
        run_import(&root, &marker_package("A"), true).unwrap();
        let real = listed_snapshot_names(&root).remove(0);
        std::os::unix::fs::symlink(root.join("backups").join(&real), root.join("backups/pre-restore-1.tar")).unwrap();
        assert_eq!(listed_snapshot_names(&root), vec![real.clone()]);
        assert!(matches!(run_rollback(&root, "pre-restore-1.tar", false, false), Err(RollbackFailure::NotFound)));
        assert!(run_rollback(&root, &real, false, false).is_ok());
        drop(dir);
    }

    /// 根目录同名 backup-manifest.json（用户文件）：不入覆盖面（写回本就跳过、
    /// 不会被覆盖）、不入快照——954 前它以裸名入快照，与快照自身 manifest 并存，
    /// 回滚时裸名查找先命中用户文件，先验失败=该快照永远不可回滚。
    #[test]
    fn root_level_manifest_file_is_neither_overwritten_nor_snapshotted() {
        let (dir, root) = temp_root();
        let user_note = r#"{"note":"用户自己的清单笔记"}"#;
        std::fs::write(root.join(MANIFEST_NAME), user_note).unwrap();
        let original = std::fs::read(root.join("books/b1/book.json")).unwrap();
        let package = marker_package("A");
        let preview = run_import(&root, &package, false).unwrap();
        assert_eq!(
            preview["preview"]["overwriting"],
            serde_json::json!(["books/b1/book.json", "inkos.json"])
        );
        run_import(&root, &package, true).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(MANIFEST_NAME)).unwrap(), user_note);
        let snapshot = listed_snapshot_names(&root).remove(0);
        let entries = parse_archive(&std::fs::read(root.join("backups").join(&snapshot)).unwrap()).unwrap().0;
        assert_eq!(entries.iter().filter(|entry| entry.name == MANIFEST_NAME).count(), 1);
        // 快照可回滚：book.json 回到恢复前原件，用户笔记原样保留。
        let rolled = run_rollback(&root, &snapshot, true, false).unwrap();
        assert_eq!(rolled["ok"], true);
        assert_eq!(std::fs::read(root.join("books/b1/book.json")).unwrap(), original);
        assert_eq!(std::fs::read_to_string(root.join(MANIFEST_NAME)).unwrap(), user_note);
        drop(dir);
    }

    #[test]
    fn import_requires_manifest_and_validates_generator() {
        let (_dir, root) = temp_root();
        let no_manifest = simple_tar(&[("inkos-backup/x.md", b"x")]);
        assert_eq!(
            run_import(&root, &no_manifest, false).unwrap_err(),
            "backup-manifest.json missing"
        );
        let bad_generator = simple_tar(&[
            (MANIFEST_NAME, &manifest_bytes("other-tool", 1)),
        ]);
        assert_eq!(
            run_import(&root, &bad_generator, false).unwrap_err(),
            "unsupported backup generator/version"
        );
        let bad_version = simple_tar(&[
            (MANIFEST_NAME, &manifest_bytes("inkos-desktop", 2)),
        ]);
        assert_eq!(
            run_import(&root, &bad_version, false).unwrap_err(),
            "unsupported backup generator/version"
        );
        let unreadable = simple_tar(&[(MANIFEST_NAME, b"not-json{")]);
        assert_eq!(
            run_import(&root, &unreadable, false).unwrap_err(),
            "backup-manifest.json unreadable"
        );
    }

    #[test]
    fn import_rejects_unsafe_entry_names_wholesale() {
        let (_dir, root) = temp_root();
        // 安全一则：任一不安全条目 → 整包 400（TS 逐字），无一落盘。
        let evil = simple_tar(&[
            (MANIFEST_NAME, &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/../evil.md", b"evil"),
            ("inkos-backup/safe.md", b"safe"),
        ]);
        let error = run_import(&root, &evil, true).unwrap_err();
        assert!(error.starts_with("unsafe entries rejected: "), "{error}");
        assert!(error.contains("evil.md"), "{error}");
        assert!(!root.join("evil.md").exists());
        assert!(!root.join("inkos-backup/safe.md").exists());
        assert_eq!(run_import(&root, &simple_tar(&[
            (MANIFEST_NAME, &manifest_bytes("inkos-desktop", 1)),
            ("/etc/passwd", b"evil"),
        ]), true).unwrap_err(), "unsafe entries rejected: /etc/passwd");
    }

    // 941 号：includeSessions 参数真化的 Rust 侧行为锁（对齐 TS 双用例）。
    #[test]
    fn export_include_sessions_true_packs_sessions() {
        let (_dir, root) = temp_root();
        std::fs::create_dir_all(root.join("sessions/sess-1")).unwrap();
        std::fs::write(root.join("sessions/sess-1/session.jsonl"), r#"{"seq":1,"type":"turn"}"#).unwrap();
        let gz = run_export(&root, false, true).unwrap();
        let entries = parse_archive(&gz).unwrap().0;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"inkos-backup/sessions/sess-1/session.jsonl"),
            "{names:?}"
        ); // 参数真化：sessions 入包
        let manifest: serde_json::Value = serde_json::from_slice(
            entries
                .iter()
                .find(|e| e.name.ends_with(MANIFEST_NAME))
                .unwrap()
                .bytes
                .as_slice(),
        )
        .unwrap();
        assert_eq!(manifest["scope"]["includeSessions"], true);
    }

    #[test]
    fn export_default_excludes_sessions() {
        let (_dir, root) = temp_root();
        std::fs::create_dir_all(root.join("sessions/sess-1")).unwrap();
        std::fs::write(root.join("sessions/sess-1/session.jsonl"), "{}").unwrap();
        let gz = run_export(&root, false, false).unwrap();
        let entries = parse_archive(&gz).unwrap().0;
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.iter().any(|n| n.starts_with("inkos-backup/sessions/")), "{names:?}"); // 默认排除保持
    }

    #[test]
    fn import_accepts_gzip_layer() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"id":"b1","gz":true}"#),
        ]);
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        std::io::Write::write_all(&mut encoder, &tar_bytes).unwrap();
        let gz = encoder.finish().unwrap();
        let response = run_import(&root, &gz, true).unwrap();
        assert_eq!(response["restored"], 1);
        let restored = std::fs::read_to_string(root.join("books/b1/book.json")).unwrap();
        assert!(restored.contains("gz"));
    }

    /// 950 号：长路径三通道回归锁。真实 works 路径
    /// `inkos-backup/works/<长id>/revisions/<长rev>/...` 超 USTAR name 字段
    /// 100 字节上限——Rust 导出侧经 GNU LongLink 表达、TS 导出侧经
    /// POSIX prefix 拆分/'L' 表达（同轮对偶改造）；双端读侧均须恢复完整名。
    #[test]
    fn export_long_paths_roundtrip_via_gnu_longname() {
        let (_dir, root) = temp_root();
        let rel = "works/w-12345678-1234-5678-1234-567812345678/revisions/r-87654321-4321-8765-4321-876543218765/chapter-0001-r2.json";
        assert!(rel.len() > 100, "{rel}");
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        std::fs::write(root.join(rel), br#"{"rev":"long-path-body"}"#).unwrap();
        // 无 work.json 的手工形态目录不得阻断导出（sync 隔离面活体）。
        let gz = run_export(&root, false, false).unwrap();
        let entries = parse_archive(&gz).unwrap().0;
        let full = format!("{PACKAGE_ROOT_NAME}/{rel}");
        let entry = entries
            .iter()
            .find(|e| e.name == full)
            .unwrap_or_else(|| panic!("long path entry missing: {full}; got {:?}", entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>()));
        assert_eq!(entry.bytes, br#"{"rev":"long-path-body"}"#.to_vec());
    }

    /// TS 写侧 POSIX USTAR prefix 拆分形态（950）：name 字段=末段、prefix
    /// 字段(345)=前段、magic "ustar\0"+"00"——tar::Archive 拼接恢复完整名
    /// （跨端互灌 TS 导出→Rust 导入；simple_tar 末尾含双零块故全内联构造）。
    #[test]
    fn import_accepts_ustar_prefix_entries() {
        let (_dir, root) = temp_root();
        let payload: &[u8] = br#"{"rev":"from-ts-prefix"}"#;
        let mut tar_bytes = tar_entry_block(
            &format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}"),
            None,
            &manifest_bytes("inkos-desktop", 1),
        );
        tar_bytes.extend_from_slice(&tar_entry_block(
            "chapter-0001-r2.json",
            Some("inkos-backup/works/w1/revisions/r1"),
            payload,
        ));
        tar_bytes.extend(std::iter::repeat_n(0u8, 1024));
        let response = run_import(&root, &tar_bytes, true).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["restored"], 1);
        let restored = std::fs::read(root.join("works/w1/revisions/r1/chapter-0001-r2.json")).unwrap();
        assert_eq!(restored, payload.to_vec());
    }

    /// 952 号：跨端快照回滚互灌（Rust 腿）——TS 端 pre-restore 快照形态
    /// （手写 tar 头无 magic 字段：257 区全零，仅 name/size/typeflag/cksum）
    /// +裸名条目+裸名 manifest（951 起）。Node 端恢复留档 → Rust 引擎回滚
    /// 场景：tar crate 须按 v7/基础头形态接受并恢复。
    #[test]
    fn import_accepts_ts_form_snapshot_rollback() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (MANIFEST_NAME, &manifest_bytes("inkos-desktop", 1)),
            ("books/b1/book.json", br#"{"id":"b1","rolled-back":true}"#),
        ]);
        let response = run_import(&root, &tar_bytes, true).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["restored"], 1); // manifest 不计
        let restored = std::fs::read_to_string(root.join("books/b1/book.json")).unwrap();
        assert!(restored.contains("rolled-back"), "{restored}");
    }

    // ── 956 号：完全回滚语义（快照 manifest 决策面 + introduced 可选删除）──

    const INTRODUCED_PATH_VECTORS: &str =
        include_str!("../../../packages/studio/src/__tests__/golden/backup-introduced-path-vectors.json");

    /// 956 号真流（覆盖 1+新增 1）confirm 后快照 manifest 决策面精确：
    /// overwriting=被覆盖名单、introduced=恢复新增名单；快照数据条目仅含
    /// 被覆盖文件（新文件保守回滚不删，不入快照）。
    #[test]
    fn snapshot_manifest_records_overwriting_and_introduced_lists() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"id":"b1","restored":true}"#),
            ("inkos-backup/notes/new-file.md", b"introduced content"),
        ]);
        let response = run_import(&root, &tar_bytes, true).unwrap();
        assert_eq!(response["snapshot"], true);
        let snapshot = listed_snapshot_names(&root).remove(0);
        let entries = parse_archive(&std::fs::read(root.join("backups").join(&snapshot)).unwrap()).unwrap().0;
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"books/b1/book.json"));
        assert!(!names.contains(&"notes/new-file.md"));
        let manifest: serde_json::Value =
            serde_json::from_slice(&entries.iter().find(|entry| entry.name == MANIFEST_NAME).unwrap().bytes).unwrap();
        assert_eq!(manifest["kind"], "pre-restore-snapshot");
        assert_eq!(manifest["overwriting"], serde_json::json!(["books/b1/book.json"]));
        assert_eq!(manifest["introduced"], serde_json::json!(["notes/new-file.md"]));
    }

    /// 956 号：纯新增恢复（全 introduced）也留快照——此前 overwriting 空=
    /// 无快照=纯新增恢复无任何回滚保险；现在 manifest 记 introduced 名单，
    /// 完全回滚=删除恢复引入文件。
    #[test]
    fn pure_addition_restore_now_snapshots_introduced_list() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/notes/only-new.md", b"only new"),
        ]);
        let response = run_import(&root, &tar_bytes, true).unwrap();
        assert_eq!(response["snapshot"], true);
        assert_eq!(response["restored"], 1);
        let snapshot = listed_snapshot_names(&root).remove(0);
        let entries = parse_archive(&std::fs::read(root.join("backups").join(&snapshot)).unwrap()).unwrap().0;
        // 快照唯一条目=自身 manifest（无被覆盖文件可留档）。
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, MANIFEST_NAME);
        let manifest: serde_json::Value = serde_json::from_slice(&entries[0].bytes).unwrap();
        assert_eq!(manifest["introduced"], serde_json::json!(["notes/only-new.md"]));
    }

    /// 956 号：回滚预览（Keep 模式）透传 introduced 清单——内容取自快照
    /// manifest 非盘面推断（现网改文件后清单不变）；import 端点（Ignore）
    /// 同字节永不透传（契约面不变）。
    #[test]
    fn rollback_preview_passes_through_introduced_list() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"v2":true}"#),
            ("inkos-backup/notes/new.md", b"new content"),
        ]);
        run_import(&root, &tar_bytes, true).unwrap();
        let snapshot = listed_snapshot_names(&root).remove(0);
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/new.md"), b"mutated").unwrap();
        let preview = run_rollback(&root, &snapshot, false, false).unwrap();
        assert_eq!(preview["preview"]["introduced"], serde_json::json!(["notes/new.md"]));
        // 同一快照字节走 import（Ignore 模式）——无 introduced 键。
        let bytes = std::fs::read(root.join("backups").join(&snapshot)).unwrap();
        let import_preview = run_import(&root, &bytes, false).unwrap();
        assert!(import_preview.get("preview").unwrap().get("introduced").is_none());
    }

    /// 956 号完全回滚闭环：写回覆盖文件回快照时刻 + 删除 introduced + 回滚
    /// 自身保险快照自洽（数据条目=被覆盖+待删删除前内容；manifest 撤销面=
    /// overwriting 被覆盖/introduced 空/removing 被删），保守回滚该保险快照
    /// 即复原待删文件（完全回滚自身也有保险）。
    #[test]
    fn full_rollback_restores_and_deletes_with_own_snapshot() {
        let (_dir, root) = temp_root();
        let original = std::fs::read(root.join("books/b1/book.json")).unwrap();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"v2":true}"#),
            ("inkos-backup/notes/new.md", b"introduced-v2"),
        ]);
        run_import(&root, &tar_bytes, true).unwrap();
        let snapshot = listed_snapshot_names(&root).remove(0);
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/new.md"), b"mutated-before-full-rollback").unwrap();

        let done = run_rollback(&root, &snapshot, true, true).unwrap();
        assert_eq!(done["ok"], true);
        assert_eq!(done["restored"], 1);
        assert_eq!(done["snapshot"], true);
        assert_eq!(done["removed"], 1);
        assert!(done.get("deleteFailed").is_none());
        assert_eq!(std::fs::read(root.join("books/b1/book.json")).unwrap(), original);
        assert!(!root.join("notes/new.md").exists());

        // 回滚自身保险快照（第 2 份）自洽。
        let names = listed_snapshot_names(&root);
        assert_eq!(names.len(), 2);
        let guard = &names[0]; // 数值倒序首位=本次回滚留档
        let entries = parse_archive(&std::fs::read(root.join("backups").join(guard)).unwrap()).unwrap().0;
        let data_names: Vec<&str> =
            entries.iter().filter(|entry| entry.name != MANIFEST_NAME).map(|entry| entry.name.as_str()).collect();
        assert_eq!(data_names, vec!["books/b1/book.json", "notes/new.md"]);
        let manifest: serde_json::Value =
            serde_json::from_slice(&entries.iter().find(|entry| entry.name == MANIFEST_NAME).unwrap().bytes).unwrap();
        assert_eq!(manifest["overwriting"], serde_json::json!(["books/b1/book.json"]));
        assert_eq!(manifest["introduced"], serde_json::json!([]));
        assert_eq!(manifest["removing"], serde_json::json!(["notes/new.md"]));
        // 保险闭环：保守回滚保险快照 → 待删文件复原（内容=删除前时刻）。
        let undone = run_rollback(&root, guard, true, false).unwrap();
        assert_eq!(undone["restored"], 2);
        assert_eq!(std::fs::read(root.join("notes/new.md")).unwrap(), b"mutated-before-full-rollback");
    }

    /// 956 号：保守回滚（不带 deleteIntroduced）语义不变——introduced 文件
    /// 保留、响应无 removed 键。
    #[test]
    fn conservative_rollback_keeps_introduced_files() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/books/b1/book.json", br#"{"v2":true}"#),
            ("inkos-backup/notes/new.md", b"new content"),
        ]);
        run_import(&root, &tar_bytes, true).unwrap();
        let snapshot = listed_snapshot_names(&root).remove(0);
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/new.md"), b"new content").unwrap();
        let done = run_rollback(&root, &snapshot, true, false).unwrap();
        assert_eq!(done["restored"], 1);
        assert!(done.get("removed").is_none());
        assert!(root.join("notes/new.md").exists());
    }

    /// 956 号：955- 旧形态快照（manifest 无 introduced）+ deleteIntroduced=1
    /// → 400 lacks（先验于盘面：写回/快照/tmp 均不发生）；同快照保守回滚
    /// 照常成功（向后兼容）。
    #[test]
    fn full_rollback_rejects_snapshot_without_introduced_list() {
        let (_dir, root) = temp_root();
        std::fs::create_dir_all(root.join("backups")).unwrap();
        let legacy = simple_tar(&[
            (
                MANIFEST_NAME,
                br#"{"version":1,"generator":"inkos-desktop","kind":"pre-restore-snapshot"}"#,
            ),
            ("books/b1/book.json", br#"{"legacy":true}"#),
        ]);
        std::fs::write(root.join("backups/pre-restore-1731100000000.tar"), &legacy).unwrap();
        let before = std::fs::read(root.join("books/b1/book.json")).unwrap();
        let err = run_rollback(&root, "pre-restore-1731100000000.tar", true, true);
        assert!(
            matches!(err, Err(RollbackFailure::Import(ref error)) if error.contains("lacks introduced list")),
            "{err:?}"
        );
        assert_eq!(std::fs::read(root.join("books/b1/book.json")).unwrap(), before);
        // 无新增快照、无 tmp 残留（backups/ 仅源快照一项）。
        assert_eq!(listed_snapshot_names(&root), vec!["pre-restore-1731100000000.tar".to_string()]);
        assert_eq!(std::fs::read_dir(root.join("backups")).unwrap().count(), 1);
        // 向后兼容：同快照保守回滚成功。
        let done = run_rollback(&root, "pre-restore-1731100000000.tar", true, false).unwrap();
        assert_eq!(done["restored"], 1);
    }

    /// 956 号：introduced 路径安全判定=双端共享 golden 向量表（include_str
    /// 同文件）；篡改快照（introduced 含穿越）Delete 模式 preview 与 confirm
    /// 同一先验 400 unsafe（写回/快照零变更），Keep 模式宽容展示（200 无
    /// introduced 键、confirm 不删）。
    #[test]
    fn full_rollback_rejects_unsafe_introduced_paths_and_matches_golden() {
        let vectors: serde_json::Value = serde_json::from_str(INTRODUCED_PATH_VECTORS).unwrap();
        for path in vectors["accept"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            assert!(is_safe_relative_path(path), "accept: {path:?}");
        }
        for path in vectors["reject"].as_array().unwrap() {
            let path = path.as_str().unwrap();
            assert!(!is_safe_relative_path(path), "reject: {path:?}");
        }
        let (_dir, root) = temp_root();
        std::fs::create_dir_all(root.join("backups")).unwrap();
        let evil = simple_tar(&[
            (
                MANIFEST_NAME,
                br#"{"version":1,"generator":"inkos-desktop","kind":"pre-restore-snapshot","introduced":["../../../outside/secret.txt"]}"#,
            ),
            ("books/b1/book.json", br#"{"evil":true}"#),
        ]);
        std::fs::write(root.join("backups/pre-restore-1731200000000.tar"), &evil).unwrap();
        let before = std::fs::read(root.join("books/b1/book.json")).unwrap();
        // Delete 模式 preview=confirm 同一先验（预览预言确认结果）。
        let preview = run_rollback(&root, "pre-restore-1731200000000.tar", false, true);
        assert!(
            matches!(preview, Err(RollbackFailure::Import(ref error)) if error.contains("unsafe introduced path")),
            "{preview:?}"
        );
        let confirm = run_rollback(&root, "pre-restore-1731200000000.tar", true, true);
        assert!(
            matches!(confirm, Err(RollbackFailure::Import(ref error)) if error.contains("unsafe introduced path")),
            "{confirm:?}"
        );
        assert_eq!(std::fs::read(root.join("books/b1/book.json")).unwrap(), before);
        assert_eq!(std::fs::read_dir(root.join("backups")).unwrap().count(), 1); // 无新增快照/tmp
        // Keep 模式：宽容展示（200 无 introduced 键），confirm 不删。
        let keep_preview = run_rollback(&root, "pre-restore-1731200000000.tar", false, false).unwrap();
        assert!(keep_preview.get("preview").unwrap().get("introduced").is_none());
        let keep_confirm = run_rollback(&root, "pre-restore-1731200000000.tar", true, false).unwrap();
        assert_eq!(keep_confirm["restored"], 1);
        assert!(keep_confirm.get("removed").is_none());
    }

    /// 956 号：待删文件在操作前已被外部删除=终态已达成——不计 removed（本次
    /// 未删任何文件，诚实计数）、无 deleteFailed、盘面无变化不留快照。
    #[test]
    fn full_rollback_skips_already_missing_introduced_files() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/notes/new.md", b"new content"),
        ]);
        run_import(&root, &tar_bytes, true).unwrap();
        let snapshot = listed_snapshot_names(&root).remove(0);
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(root.join("notes/new.md"), b"new content").unwrap();
        std::fs::remove_file(root.join("notes/new.md")).unwrap();
        let done = run_rollback(&root, &snapshot, true, true).unwrap();
        assert_eq!(done["removed"], 0);
        assert!(done.get("deleteFailed").is_none());
        assert_eq!(done["snapshot"], false); // 无覆盖/新建/删除=不留快照
        assert_eq!(listed_snapshot_names(&root).len(), 1);
    }

    /// 956 号（unix）：待删名字为符号链接时不跟随不删（删除面=常规文件，与
    /// 955 回滚面同判据）——链接保留、目标不动、deleteFailed 明示。
    #[test]
    #[cfg(unix)]
    fn full_rollback_skips_symlinked_introduced_targets() {
        let (_dir, root) = temp_root();
        let tar_bytes = simple_tar(&[
            (format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}").as_str(), &manifest_bytes("inkos-desktop", 1)),
            ("inkos-backup/notes/link.md", b"placeholder"),
        ]);
        run_import(&root, &tar_bytes, true).unwrap();
        let snapshot = listed_snapshot_names(&root).remove(0);
        std::fs::create_dir_all(root.join("notes")).unwrap();
        let target = root.join("notes/real-target.txt");
        std::fs::write(&target, b"keep me").unwrap();
        std::fs::remove_file(root.join("notes/link.md")).unwrap(); // 恢复写出的常规文件
        std::os::unix::fs::symlink(&target, root.join("notes/link.md")).unwrap();
        let done = run_rollback(&root, &snapshot, true, true).unwrap();
        assert_eq!(done["removed"], 0);
        let failed = done["deleteFailed"].as_array().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0]["path"], "notes/link.md");
        assert_eq!(failed[0]["error"], "not a regular file");
        assert!(root.join("notes/link.md").exists()); // 链接本身保留
        assert_eq!(std::fs::read(&target).unwrap(), b"keep me"); // 目标不动
    }

    /// 950 号：单条目 tar 块（512B 头+payload+补齐），支持 POSIX prefix 字段
    /// 与任意 typeflag——`simple_tar` 的可变体（其末尾双零块使拼接需分块构造）。
    fn tar_entry_block(name: &str, prefix: Option<&str>, payload: &[u8]) -> Vec<u8> {
        let mut header = vec![0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:011o}", payload.len());
        header[124..124 + size.len()].copy_from_slice(size.as_bytes());
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        if let Some(prefix) = prefix {
            header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        }
        for byte in &mut header[148..156] {
            *byte = b' ';
        }
        let sum: u32 = header.iter().map(|&b| b as u32).sum();
        let octal = format!("{:06o}\0 ", sum);
        header[148..156].copy_from_slice(octal.as_bytes());
        let mut out = header;
        out.extend_from_slice(payload);
        let padding = (512 - (payload.len() % 512)) % 512;
        out.extend(std::iter::repeat_n(0u8, padding));
        out
    }
}
