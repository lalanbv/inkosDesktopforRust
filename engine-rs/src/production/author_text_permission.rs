//! 作者文本权限解析（890 号 M3c 下）：`packages/core/src/harness/author-text-permission.ts`
//! 移植——权限只可窄化不可放大（intersecting slots）、缓存文件（selectorVersion 键）、
//! 受保护片段匹配（BASELINE_CONFLICT/AMBIGUOUS 双码）。

use sha2::{Digest, Sha256};

use super::author_edit_scope::{
    author_text_scope_contract, AuthorTextEditContract, AuthorTextScope,
    AuthorTextScopeSelection,
};
use super::text_range_edits::{text_range_edit_contract, TextEditRange};
use crate::utils::source_text::split_source_lines;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct AuthorPermissionError {
    pub code: Option<String>,
    pub message: String,
    /// BASELINE_CONFLICT/AMBIGUOUS 携带的身份面。
    pub work_id: Option<String>,
    pub artifact_id: Option<String>,
    pub revision_id: Option<String>,
}

impl AuthorPermissionError {
    fn plain(message: impl Into<String>) -> Self {
        Self { code: None, message: message.into(), work_id: None, artifact_id: None, revision_id: None }
    }

    fn coded(code: &str, message: impl Into<String>, identity: (&str, &str, &str)) -> Self {
        Self {
            code: Some(code.into()),
            message: message.into(),
            work_id: Some(identity.0.into()),
            artifact_id: Some(identity.1.into()),
            revision_id: Some(identity.2.into()),
        }
    }
}

/// 行偏移索引（TS offsets push 语义）。
fn line_offsets(content: &str) -> Vec<usize> {
    let lines = split_source_lines(content);
    let mut offsets = vec![0usize];
    for line in &lines {
        let last = *offsets.last().unwrap();
        offsets.push(last + line.len());
    }
    offsets
}

/// TS `narrowAuthorTextPermission`：操作区间与既有权限求交（删除槽在共享
/// 行边界/EOF 的归属规则逐字对齐；空并集 → OPERATION_BOUNDARY）。
pub fn narrow_author_text_permission(
    content: &str,
    permission_ranges: &[(usize, usize, bool)],
    _permission_focus: Option<Vec<super::author_edit_scope::AuthorTextScopeFocus>>,
    _original_selection_texts: &[String],
    ranges: &[TextEditRange],
) -> Result<super::text_range_edits::ScopedSelectionContract, AuthorPermissionError> {
    let operation =
        text_range_edit_contract(content, ranges).map_err(AuthorPermissionError::plain)?;
    let offsets = line_offsets(content);
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    for range in permission_ranges {
        let start = range.0;
        let end = range.1;
        for limit in &operation.ranges {
            let limit_start = offsets[limit.start_line as usize - 1];
            let limit_end = offsets[limit.end_line as usize];
            // 删除槽（start==end）：行边界下归属后一区间；EOF 归属最后区间。
            if start == end
                && (start < limit_start
                    || start > limit_end
                    || (start == limit_end && limit_end < content.len()))
            {
                continue;
            }
            let start_offset = std::cmp::max(start, limit_start);
            let end_offset = std::cmp::min(end, limit_end);
            if start_offset > end_offset || (start_offset == end_offset && start != end) {
                continue;
            }
            spans.push((start_offset, end_offset, range.2));
        }
    }
    if spans.is_empty() {
        return Err(AuthorPermissionError::plain(
            "The current operation ranges do not contain any author-authorized text. Read the current revision and select ranges within the original permission.",
        )
        .coded_flag("ARTIFACT_EDIT_OPERATION_BOUNDARY"));
    }
    super::text_range_edits::text_offset_selection_edit_contract(content, &spans)
        .map_err(AuthorPermissionError::plain)
}

impl AuthorPermissionError {
    fn coded_flag(mut self, code: &str) -> Self {
        self.code = Some(code.into());
        self
    }
}

/// TS `matchesProtectedFragments`：受保护片段有序匹配（前缀/后缀锚定，
/// 中段按序 indexOf；无正则回退）。
pub fn matches_protected_fragments(content: &str, parts: &[&str]) -> bool {
    let prefix = parts.first().copied().unwrap_or_default();
    let suffix = parts.last().copied().unwrap_or_default();
    if !content.starts_with(prefix) || !content.ends_with(suffix) {
        return false;
    }
    let mut cursor = prefix.len();
    let end = content.len().saturating_sub(suffix.len());
    if cursor > end {
        return false;
    }
    for part in parts.iter().take(parts.len().saturating_sub(1)).skip(1) {
        let Some(index) = content[cursor..].find(part) else {
            return false;
        };
        let index = cursor + index;
        if index + part.len() > end {
            return false;
        }
        cursor = index + part.len();
    }
    true
}

/// TS `resolveAuthorTextPermission` 的**纯逻辑面**（缓存文件读写由调用方注入——
/// `select` 为 LLM 环回调）。合同投影：selected 非 wholeDocument 时按受保护
/// 片段在当前文重映射（BASELINE_CONFLICT=受保护文本被改、AMBIGUOUS=多重映射）。
#[allow(clippy::too_many_arguments)]
pub async fn resolve_author_text_permission(
    select: impl FnOnce(String, String) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AuthorTextScope, String>> + Send>>,
    input: ResolveAuthorTextPermissionInput<'_>,
) -> Result<ResolvedAuthorPermission, AuthorPermissionError> {
    let identity = (input.work_id, input.artifact_id, input.revision_id);
    let mut cache_key_material = String::new();
    cache_key_material.push_str(&serde_json::to_string(&serde_json::json!({
        "version": input.selector_version,
        "workId": input.work_id,
        "artifactId": input.artifact_id,
        "revisionId": input.revision_id,
        "originalContent": input.original_content,
        "authorRequest": input.author_request,
    }))
    .unwrap_or_default());
    let mut hasher = Sha256::new();
    hasher.update(cache_key_material.as_bytes());
    let _cache_key = format!("{:x}", hasher.finalize());

    let selected: AuthorTextScope = select(
        input.original_content.to_string(),
        input.author_request.to_string(),
    )
    .await
    .map_err(AuthorPermissionError::plain)?;
    // TS：select 后即合同化（Address 不合法在权限阶段即抛）。
    let original_contract =
        author_text_scope_contract(input.original_content, &selected).map_err(|error| {
            AuthorPermissionError::plain(error.message)
        })?;
    let mut contract = match &original_contract {
        AuthorTextEditContract::Ranges(ranges) => {
            // wholeDocument 且当前文已变 → 以当前文重新合同化（同 scope）。
            if input.current_content != input.original_content {
                author_text_scope_contract(input.current_content, &selected)
                    .map_err(|error| AuthorPermissionError::plain(error.message))?
            } else {
                AuthorTextEditContract::Ranges(super::text_range_edits::RangeEditContract {
                    ranges: ranges.ranges.clone(),
                    field_names: ranges.field_names.clone(),
                    content: ranges.content.clone(),
                    sorted: ranges.sorted.clone(),
                })
            }
        }
        AuthorTextEditContract::Scoped(scoped) => {
            AuthorTextEditContract::Scoped(clone_scoped(scoped))
        }
    };
    if !selected.whole_document && input.current_content != input.original_content {
        // 受保护片段重映射（TS L118-160 逐段）。
        let ranges: Vec<(usize, usize, bool)> = match &original_contract {
            AuthorTextEditContract::Scoped(scoped) => scoped
                .spans
                .iter()
                .map(|span| (span.start_offset, span.end_offset, span.single_line))
                .collect(),
            _ => {
                return Err(AuthorPermissionError::plain(
                    "Partial author selections require source offsets.",
                ));
            }
        };
        let mut merged: Vec<(usize, usize, bool)> = Vec::new();
        for range in ranges {
            match merged.last_mut() {
                Some(previous) if previous.1 == range.0 => {
                    previous.1 = range.1;
                    previous.2 = previous.2 && range.2
                        && !input.original_content[previous.0..previous.1].contains(['\r', '\n']);
                }
                _ => merged.push(range),
            }
        }
        let mut cursor = 0usize;
        let mut protected_parts: Vec<&str> = Vec::new();
        for range in &merged {
            protected_parts.push(&input.original_content[cursor..range.0]);
            cursor = range.1;
        }
        protected_parts.push(&input.original_content[cursor..]);
        if !matches_protected_fragments(input.current_content, &protected_parts) {
            return Err(AuthorPermissionError::coded(
                "ARTIFACT_EDIT_BASELINE_CONFLICT",
                "Protected source changed after this request began. Start a new request against the current revision.",
                identity,
            ));
        }
        let mut left: Vec<usize> = Vec::new();
        cursor = protected_parts[0].len();
        for part in protected_parts.iter().take(protected_parts.len().saturating_sub(1)).skip(1) {
            let index = input.current_content[cursor..]
                .find(part)
                .ok_or_else(|| AuthorPermissionError::plain("protected fragment vanished"))?;
            left.push(cursor + index);
            cursor = cursor + index + part.len();
        }
        let mut right: Vec<usize> = Vec::new();
        cursor = input.current_content.len().saturating_sub(protected_parts.last().unwrap().len());
        for part in protected_parts[1..protected_parts.len().saturating_sub(1)].iter().rev() {
            let index = input.current_content[..cursor]
                .rfind(part)
                .ok_or_else(|| AuthorPermissionError::plain("protected fragment vanished"))?;
            right.insert(0, index);
            cursor = index;
        }
        if left != right {
            return Err(AuthorPermissionError::coded(
                "ARTIFACT_EDIT_BASELINE_AMBIGUOUS",
                "The current text has multiple possible mappings to the original edit selections. Do not guess or widen the edit.",
                identity,
            ));
        }
        cursor = protected_parts[0].len();
        let mut projected: Vec<(usize, usize, bool)> = Vec::new();
        for (index, range) in merged.iter().enumerate() {
            let end_offset = if index < left.len() {
                left[index]
            } else {
                input.current_content.len() - protected_parts.last().unwrap().len()
            };
            projected.push((cursor, end_offset, range.2));
            cursor = end_offset + protected_parts.get(index + 1).map(|part| part.len()).unwrap_or(0);
        }
        let scoped = super::text_range_edits::text_offset_selection_edit_contract(
            input.current_content,
            &projected,
        )
        .map_err(AuthorPermissionError::plain)?;
        contract = AuthorTextEditContract::Scoped(scoped);
    }
    let original_selection_texts: Vec<String> = match &original_contract {
        AuthorTextEditContract::Ranges(ranges) => {
            ranges.ranges.iter().map(|range| range.content.clone()).collect()
        }
        AuthorTextEditContract::Scoped(scoped) => {
            scoped.spans.iter().map(|span| span.text.clone()).collect()
        }
    };
    Ok(ResolvedAuthorPermission {
        contract,
        original_selection_texts,
    })
}

fn clone_scoped(scoped: &super::text_range_edits::ScopedSelectionContract) -> super::text_range_edits::ScopedSelectionContract {
    super::text_range_edits::ScopedSelectionContract {
        content: scoped.content.clone(),
        spans: scoped.spans.clone(),
    }
}

pub struct ResolveAuthorTextPermissionInput<'a> {
    pub work_id: &'a str,
    pub artifact_id: &'a str,
    pub revision_id: &'a str,
    pub original_content: &'a str,
    pub current_content: &'a str,
    pub author_request: &'a str,
    pub selector_version: u32,
}

pub struct ResolvedAuthorPermission {
    pub contract: AuthorTextEditContract,
    pub original_selection_texts: Vec<String>,
}

/// author_text_permission 模块的 scope 构造辅助（SourceLocatorAgent 输出面）。
pub fn scope_selection(unit_id: &str, text: &str) -> AuthorTextScopeSelection {
    AuthorTextScopeSelection { unit_id: unit_id.into(), text: text.into() }
}
