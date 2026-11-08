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
use std::path::Path;
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

fn run_import(root: &Path, body: &[u8], confirm: bool) -> Result<serde_json::Value, String> {
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
    // 将覆盖项 = root 下已存在的同名文件。
    let overwriting: Vec<String> = entries
        .iter()
        .map(|entry| restore_name(&entry.name))
        .filter(|name| is_existing(&root.join(name)))
        .collect();
    if !confirm {
        let new_files = entries.len() - overwriting.len();
        return Ok(json!({
            "preview": {
                "fileCount": entries.len(),
                "overwriting": overwriting,
                "newFiles": new_files,
            }
        }));
    }

    // 恢复前自动快照：将覆盖的现有文件打包留存（回滚保险）。
    let has_snapshot = !overwriting.is_empty();
    if has_snapshot {
        let backups_dir = root.join("backups");
        std::fs::create_dir_all(&backups_dir).map_err(|e| e.to_string())?;
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
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
            // 951 号：快照附带 manifest（裸名形态，双端 import 端点均兼容
            // 裸名查找与写回跳过）——pre-restore 快照可直接经
            // backup/import?confirm=1 回灌=回滚保险从「留档」到「可执行」
            // 闭环。部分回滚边界：恢复引入的新文件不在快照（保守不引入
            // 删除语义，备案）。
            let snapshot_manifest = serde_json::json!({
                "version": 1,
                "generator": "inkos-desktop",
                "kind": "pre-restore-snapshot",
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
        if entry.name == format!("{PACKAGE_ROOT_NAME}/{MANIFEST_NAME}") || entry.name == MANIFEST_NAME {
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
    Ok(json!({ "ok": true, "restored": restored, "snapshot": has_snapshot }))
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
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
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
        assert_eq!(preview["fileCount"], 3); // manifest 元数据计入包内文件数（TS 口径）
        assert_eq!(preview["overwriting"], serde_json::json!(["books/b1/book.json"]));
        assert_eq!(preview["newFiles"], 2);
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
