//! 项目级上传落盘共享件（273 号）。
//!
//! TS `safeUploadFileName`（server.ts L621）/ `parseDataUrl`（L641）/
//! `storeProjectUpload`（L739）逐字移植，供正典上传（`/import/canon/upload`）
//! 使用；翻译上传（80MB 专属错误文案）暂留 translation_routes 自持实现，
//! 为后续收敛候选。base64 宽松解码复用 skill_routes 的
//! `decode_base64_lenient`（Node `Buffer.from(s, "base64")` 语义）。
//!
//! 错误形态对齐 TS `ApiError` → 全局 onError：`{error:{code,message}}`。

use std::path::Path;

use axum::http::StatusCode;
use axum::Json;
use serde_json::json;

use crate::interaction::session::utc_now_ms;
use crate::server::skill_routes::decode_base64_lenient;

pub(crate) type UploadApiError = (StatusCode, Json<serde_json::Value>);

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> UploadApiError {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message.into() } })),
    )
}

/// TS `safeUploadFileName`：trim → `[/\\\0]→_` → `\s+→" "` →
/// `[^\p{L}\p{N}._ -]+→_`（连续段合一个 `_`）→ `slice(0,120)`（UTF-16 码元）
/// → trim → 兜底 `"upload"`。
pub(crate) fn safe_upload_filename(value: &str) -> String {
    let trimmed = value.trim().replace(['/', '\\', '\0'], "_");
    let mut collapsed = String::with_capacity(trimmed.len());
    let mut in_ws = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !in_ws {
                collapsed.push(' ');
                in_ws = true;
            }
        } else {
            collapsed.push(ch);
            in_ws = false;
        }
    }
    let mut mapped = String::with_capacity(collapsed.len());
    let mut in_run = false;
    for ch in collapsed.chars() {
        let allowed =
            ch.is_alphabetic() || ch.is_numeric() || matches!(ch, '.' | '_' | ' ' | '-');
        if allowed {
            mapped.push(ch);
            in_run = false;
        } else if !in_run {
            mapped.push('_');
            in_run = true;
        }
    }
    let sliced = truncate_utf16(&mapped, 120);
    let safe = sliced.trim();
    if safe.is_empty() {
        "upload".to_string()
    } else {
        safe.to_string()
    }
}

fn truncate_utf16(value: &str, max: usize) -> String {
    let mut units = 0usize;
    let mut out = String::new();
    for ch in value.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > max {
            break;
        }
        units += ch_units;
        out.push(ch);
    }
    out
}

fn invalid_data_url() -> UploadApiError {
    api_error(
        StatusCode::BAD_REQUEST,
        "INVALID_ATTACHMENT_DATA_URL",
        "Attachment must be a base64 data URL",
    )
}

/// TS `parseDataUrl`：`^data:([^;,]+)?(?:;[^,]*)?;base64,(.*)$`（dotall）。
/// mime 段与参数段都禁止逗号，`;base64,` 匹配点之前不得出现逗号；payload
/// 可含逗号与换行。mime 缺省（空/未提供）→ `application/octet-stream`。
pub(crate) fn parse_data_url_with_mime(data_url: &str) -> Result<(Vec<u8>, String), UploadApiError> {
    let Some(rest) = data_url.strip_prefix("data:") else {
        return Err(invalid_data_url());
    };
    let Some(marker) = rest.find(";base64,") else {
        return Err(invalid_data_url());
    };
    let head = &rest[..marker];
    if head.contains(',') {
        return Err(invalid_data_url());
    }
    let mime = head.split(';').next().unwrap_or("").trim();
    let mime = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime.to_string()
    };
    Ok((
        decode_base64_lenient(&rest[marker + ";base64,".len()..]),
        mime,
    ))
}

/// `storeProjectUpload` 成功产物（camelCase 响应字段）。
#[derive(Debug)]
pub(crate) struct StoreProjectUpload {
    pub stored_path: String,
    pub size: usize,
    pub mime_type: String,
}

/// TS `storeProjectUpload`：文件名清洗 → dataUrl 缺失 400 → 解析 →
/// 解码后字节上限 413 → `.inkos/uploads/{scope}/{毫秒}-{name}` 落盘 →
/// 返回相对 root 的 posix 路径。
pub(crate) async fn store_project_upload(
    root: &Path,
    filename: Option<&str>,
    data_url: Option<&str>,
    scope: &str,
    fallback_name: &str,
    max_bytes: usize,
    error_code: &str,
) -> Result<StoreProjectUpload, UploadApiError> {
    let name = safe_upload_filename(filename.unwrap_or(fallback_name));
    let Some(data_url) = data_url else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            error_code,
            "Upload is missing dataUrl",
        ));
    };
    let (buffer, mime_type) = parse_data_url_with_mime(data_url)?;
    if buffer.len() > max_bytes {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("{error_code}_TOO_LARGE").as_str(),
            format!("{name} exceeds {max_bytes} bytes"),
        ));
    }
    let upload_dir = root
        .join(".inkos")
        .join("uploads")
        .join(safe_upload_filename(scope));
    tokio::fs::create_dir_all(&upload_dir)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "failed to create upload directory"))?;
    let stored_name = format!("{}-{name}", utc_now_ms());
    let stored_path = upload_dir.join(&stored_name);
    tokio::fs::write(&stored_path, &buffer)
        .await
        .map_err(|_| api_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "failed to store upload"))?;
    let stored_rel = stored_path
        .strip_prefix(root)
        .unwrap_or(&stored_path)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(StoreProjectUpload {
        stored_path: stored_rel,
        size: buffer.len(),
        mime_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_upload_filename_ts_cases() {
        // 路径分隔与空字节 → 下划线；空白折叠；非法字符段合并为单下划线
        assert_eq!(safe_upload_filename("a/b\\c.d.txt"), "a_b_c.d.txt");
        assert_eq!(safe_upload_filename("  hello   world  "), "hello world");
        assert_eq!(safe_upload_filename("原神◎※设定"), "原神_设定");
        // 120 UTF-16 码元截断（汉字 1 码元/字）
        let long = "字".repeat(200);
        assert_eq!(safe_upload_filename(&long).chars().count(), 120);
        // '_' 在允许类内——"///" 映射为 "___"（TS truthy，不触发兜底）
        assert_eq!(safe_upload_filename("///"), "___");
        // trim 后为空 → 兜底
        assert_eq!(safe_upload_filename("  "), "upload");
        assert_eq!(safe_upload_filename(""), "upload");
    }

    #[test]
    fn parse_data_url_ts_cases() {
        let (bytes, mime) = parse_data_url_with_mime("data:text/plain;base64,aGVsbG8=").unwrap();
        assert_eq!(bytes, b"hello");
        assert_eq!(mime, "text/plain");
        // 参数段跳过；mime 取 ';' 之前
        let (_, mime) =
            parse_data_url_with_mime("data:application/json;charset=utf-8;base64,e30=").unwrap();
        assert_eq!(mime, "application/json");
        // mime 缺省 → octet-stream
        let (_, mime) = parse_data_url_with_mime("data:;base64,QQ==").unwrap();
        assert_eq!(mime, "application/octet-stream");
        // 非 base64 data URL / 缺前缀 / mime 段含逗号 → 400
        assert!(parse_data_url_with_mime("data:text/plain,hello").is_err());
        assert!(parse_data_url_with_mime("hello").is_err());
        assert!(parse_data_url_with_mime("data:,x;base64,QQ==").is_err());
        // payload 可含换行（dotall）
        let (bytes, _) = parse_data_url_with_mime("data:;base64,QQ==\n").unwrap();
        assert_eq!(bytes, b"A");
    }

    #[tokio::test]
    async fn store_project_upload_roundtrip_and_caps() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();
        let outcome = store_project_upload(
            &root_path,
            Some("my canon/设定.txt"),
            Some("data:text/plain;base64,aGVsbG8="),
            "canon",
            "canon-source",
            18 * 1024 * 1024,
            "INVALID_CANON_UPLOAD",
        )
        .await
        .unwrap();
        assert_eq!(outcome.mime_type, "text/plain");
        assert_eq!(outcome.size, 5);
        assert!(outcome.stored_path.starts_with(".inkos/uploads/canon/"));
        assert!(outcome.stored_path.ends_with("-my canon_设定.txt"));
        let full = root_path.join(&outcome.stored_path);
        assert_eq!(tokio::fs::read(&full).await.unwrap(), b"hello");

        // 缺 dataUrl → 400 INVALID_CANON_UPLOAD
        let err = store_project_upload(
            &root_path,
            None,
            None,
            "canon",
            "canon-source",
            1024,
            "INVALID_CANON_UPLOAD",
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        // 超上限 → 413 INVALID_CANON_UPLOAD_TOO_LARGE
        let err = store_project_upload(
            &root_path,
            Some("big.bin"),
            Some("data:;base64,QUJD"),
            "canon",
            "canon-source",
            2,
            "INVALID_CANON_UPLOAD",
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, StatusCode::PAYLOAD_TOO_LARGE);
        // 非法 dataUrl → 400 INVALID_ATTACHMENT_DATA_URL
        let err = store_project_upload(
            &root_path,
            None,
            Some("not-a-data-url"),
            "canon",
            "canon-source",
            1024,
            "INVALID_CANON_UPLOAD",
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }
}
