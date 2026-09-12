//! R7 结构化错误目录（368 号契约层，二轮 P2）。
//!
//! TS 真源：`packages/core/src/utils/author-error-catalog.ts`；数据文件
//! `packages/core/src/data/author-errors.json`（`include_str!` 同一文件，
//! 双端共享同一错误→分类→下一步映射，author-report/通知中心/Doctor 三面
//! 共用）。行为 golden = `golden/author-error-catalog-vectors.json`。
//!
//! 移植纪律：patterns 匹配大小写不敏感（haystack 与 pattern 均小写
//! includes）；目录顺序即优先级；无命中走末条兜底（patterns 为空）。

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorErrorEntry {
    pub code: String,
    pub severity: String,
    #[serde(default)]
    pub patterns: Vec<String>,
    pub message_zh: String,
    pub message_en: String,
    pub next_action_zh: String,
    pub next_action_en: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorErrorCatalog {
    pub version: i64,
    pub errors: Vec<AuthorErrorEntry>,
}

pub const AUTHOR_ERRORS_JSON: &str =
    include_str!("../../../packages/core/src/data/author-errors.json");

pub fn load_author_error_catalog() -> AuthorErrorCatalog {
    serde_json::from_str(AUTHOR_ERRORS_JSON).expect("author-errors.json should be valid")
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedAuthorError {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub next_action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogLanguage {
    Zh,
    En,
}

/// 目录查找：patterns 顺序匹配（haystack 小写 includes）；无命中走兜底条目。
pub fn resolve_author_error(
    event: &str,
    message: &str,
    language: CatalogLanguage,
) -> ResolvedAuthorError {
    let catalog = load_author_error_catalog();
    let is_en = language == CatalogLanguage::En;
    let haystack = format!("{} {}", event, message).to_lowercase();
    let matched = catalog
        .errors
        .iter()
        .find(|entry| {
            entry
                .patterns
                .iter()
                .any(|pattern| haystack.contains(&pattern.to_lowercase()))
        })
        .or_else(|| catalog.errors.iter().find(|entry| entry.patterns.is_empty()))
        .or_else(|| catalog.errors.last());
    let entry = match matched {
        Some(entry) => entry,
        None => {
            return ResolvedAuthorError {
                code: "unknown-error".to_string(),
                severity: "must-handle".to_string(),
                message: if is_en { "The run failed and needs manual attention.".to_string() } else { "任务失败，需人工介入。".to_string() },
                next_action: if is_en { "Check Doctor diagnostics and logs, fix the cause, then re-run.".to_string() } else { "查看 Doctor 诊断与日志定位原因，修复后重跑。".to_string() },
            };
        }
    };
    ResolvedAuthorError {
        code: entry.code.clone(),
        severity: entry.severity.clone(),
        message: if is_en { entry.message_en.clone() } else { entry.message_zh.clone() },
        next_action: if is_en { entry.next_action_en.clone() } else { entry.next_action_zh.clone() },
    }
}

/// 按 severity 取下一步建议（未知 severity 走兜底条目）。
pub fn next_action_for(severity: &str, language: CatalogLanguage) -> String {
    let catalog = load_author_error_catalog();
    let is_en = language == CatalogLanguage::En;
    match catalog.errors.iter().find(|entry| entry.severity == severity) {
        Some(entry) => {
            if is_en { entry.next_action_en.clone() } else { entry.next_action_zh.clone() }
        }
        None => {
            let fallback = catalog.errors.last().expect("catalog has fallback entry");
            if is_en { fallback.next_action_en.clone() } else { fallback.next_action_zh.clone() }
        }
    }
}
