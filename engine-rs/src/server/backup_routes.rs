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

const BACKUP_ROOT_ENTRIES: [&str; 4] = ["books", "inkos.json", ".inkos", "prompt"];
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

/// `GET /api/v1/backup/export`——全量 tar.gz 下载。
pub async fn export(
    State(runtime): State<BooksRuntime>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let include_secrets = params.get("includeSecrets").map(|v| v == "1").unwrap_or(false);
    let include_sessions = params.get("includeSessions").map(|v| v == "1").unwrap_or(false);
    let root = runtime.state.project_root().to_path_buf();
    match run_export(&root, include_secrets, include_sessions) {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/gzip"),
                (header::CONTENT_DISPOSITION, "attachment; filename=\"inkos-backup.tar.gz\""),
            ],
            bytes,
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn run_export(root: &Path, include_secrets: bool, include_sessions: bool) -> std::io::Result<Vec<u8>> {
    let _ = include_sessions; // scope 记录用；打包范围与 TS 一致不含 sessions/
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
    if std::fs::write(staging.join(MANIFEST_NAME), serde_json::to_vec_pretty(&manifest).unwrap_or_default()).is_err() {
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
    match run_import(&root, &body, confirm) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
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
        let file = std::fs::File::create(&snapshot_path).map_err(|e| e.to_string())?;
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
        builder.finish().map_err(|e| e.to_string())?;
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
        std::fs::write(&target, &entry.bytes).map_err(|e| e.to_string())?;
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
        assert_eq!(snapshot_entries.len(), 1);
        assert_eq!(snapshot_entries[0].name, "books/b1/book.json");
        assert_eq!(snapshot_entries[0].bytes, original);
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
}
