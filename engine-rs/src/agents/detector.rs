//! AIGC 检测（detector）。
//!
//! 移植自 `packages/core/src/agents/detector.ts`（120 行）。调用外部检测 API（GPTZero/Originality/custom），
//! 返回归一化 score（0=人, 1=AI）。
//!
//! ## 架构
//! - **纯解析内核**（可单测）：[`parse_gptzero_score`] / [`parse_originality_score`] / [`parse_custom_score`]——
//!   把各 provider 的 JSON 响应 `Value → score` 提取，是 load-bearing 逻辑。
//! - **HTTP 编排**（[`detect_ai_content`]）：reqwest::Client 注入（可测试性），
//!   `detected_at` 由调用方注入（纯内核范式）。

use crate::models::project::{DetectionConfig, DetectionProvider};

/// 检测结果。对齐 TS `DetectionResult`。
#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub score: f64, // 0.0-1.0，越高越像 AI
    pub provider: String,
    pub detected_at: String,
    pub raw: Option<serde_json::Value>,
}

/// GPTZero 响应解析：`documents[0].completely_generated_prob`。对齐 TS。
///
/// 缺字段/类型错 → 0.0（与 TS `?? 0` 一致）。
pub fn parse_gptzero_score(data: &serde_json::Value) -> f64 {
    data.get("documents")
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("completely_generated_prob"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

/// Originality 响应解析：`score.ai`。对齐 TS。
pub fn parse_originality_score(data: &serde_json::Value) -> f64 {
    data.get("score")
        .and_then(|s| s.get("ai"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

/// Custom 响应解析：顶层 `score`（须为 number）。对齐 TS（非 number → 0）。
pub fn parse_custom_score(data: &serde_json::Value) -> f64 {
    match data.get("score") {
        Some(v) if v.is_number() => v.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// 调用外部检测 API。对齐 TS `detectAIContent`。
///
/// `client` 注入便于测试；`detected_at` 由调用方注入（ISO8601）。API key 从 `api_key` 参数传入
/// （TS 从 `process.env[config.apiKeyEnv]` 读——Rust 由调用方解析环境变量后传入，纯内核不读 env）。
pub async fn detect_ai_content(
    client: &reqwest::Client,
    config: &DetectionConfig,
    content: &str,
    api_key: &str,
    detected_at: &str,
) -> Result<DetectionResult, DetectError> {
    let result = match config.provider {
        DetectionProvider::Gptzero => {
            detect_gptzero(client, &config.api_url, api_key, content, detected_at).await?
        }
        DetectionProvider::Originality => {
            detect_originality(client, &config.api_url, api_key, content, detected_at).await?
        }
        DetectionProvider::Custom => {
            detect_custom(client, &config.api_url, api_key, content, detected_at).await?
        }
    };
    Ok(result)
}

/// 检测错误（HTTP 失败 / 解析失败）。对齐 TS 抛出的 Error。
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("{provider} API failed: {status} {body}")]
    Http { provider: &'static str, status: u16, body: String },
    #[error("{provider} response parse failed: {source}")]
    Parse { provider: &'static str, source: reqwest::Error },
    #[error("request transport: {0}")]
    Transport(#[from] reqwest::Error),
}

async fn detect_gptzero(
    client: &reqwest::Client,
    api_url: &str,
    api_key: &str,
    content: &str,
    detected_at: &str,
) -> Result<DetectionResult, DetectError> {
    let resp = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("X-Api-Key", api_key)
        .json(&serde_json::json!({ "document": content }))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(DetectError::Http { provider: "GPTZero", status: status.as_u16(), body });
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| DetectError::Parse {
        provider: "GPTZero",
        source: e,
    })?;
    let score = parse_gptzero_score(&data);
    Ok(DetectionResult {
        score,
        provider: "gptzero".to_string(),
        detected_at: detected_at.to_string(),
        raw: Some(data),
    })
}

async fn detect_originality(
    client: &reqwest::Client,
    api_url: &str,
    api_key: &str,
    content: &str,
    detected_at: &str,
) -> Result<DetectionResult, DetectError> {
    let resp = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({ "content": content }))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(DetectError::Http { provider: "Originality", status: status.as_u16(), body });
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| DetectError::Parse {
        provider: "Originality",
        source: e,
    })?;
    let score = parse_originality_score(&data);
    Ok(DetectionResult {
        score,
        provider: "originality".to_string(),
        detected_at: detected_at.to_string(),
        raw: Some(data),
    })
}

async fn detect_custom(
    client: &reqwest::Client,
    api_url: &str,
    api_key: &str,
    content: &str,
    detected_at: &str,
) -> Result<DetectionResult, DetectError> {
    let resp = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({ "content": content }))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(DetectError::Http { provider: "Detection", status: status.as_u16(), body });
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| DetectError::Parse {
        provider: "Custom",
        source: e,
    })?;
    let score = parse_custom_score(&data);
    Ok(DetectionResult {
        score,
        provider: "custom".to_string(),
        detected_at: detected_at.to_string(),
        raw: Some(data),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gptzero_score_extracts_first_doc_prob() {
        let data = serde_json::json!({
            "documents": [{ "completely_generated_prob": 0.87 }, { "completely_generated_prob": 0.1 }]
        });
        assert_eq!(parse_gptzero_score(&data), 0.87);
    }

    #[test]
    fn parse_gptzero_score_defaults_zero_on_missing() {
        assert_eq!(parse_gptzero_score(&serde_json::json!({})), 0.0);
        assert_eq!(parse_gptzero_score(&serde_json::json!({ "documents": [] })), 0.0);
        assert_eq!(
            parse_gptzero_score(&serde_json::json!({ "documents": [{ "other": 1 }] })),
            0.0
        );
    }

    #[test]
    fn parse_originality_score_extracts_score_ai() {
        let data = serde_json::json!({ "score": { "ai": 0.42, "human": 0.58 } });
        assert_eq!(parse_originality_score(&data), 0.42);
    }

    #[test]
    fn parse_originality_score_defaults_zero_on_missing() {
        assert_eq!(parse_originality_score(&serde_json::json!({})), 0.0);
        assert_eq!(parse_originality_score(&serde_json::json!({ "score": {} })), 0.0);
    }

    #[test]
    fn parse_custom_score_extracts_top_level_number() {
        assert_eq!(parse_custom_score(&serde_json::json!({ "score": 0.5 })), 0.5);
        assert_eq!(parse_custom_score(&serde_json::json!({ "score": 1 })), 1.0);
    }

    #[test]
    fn parse_custom_score_rejects_non_number() {
        // 非数字 score → 0（对齐 TS `typeof === "number"` 检查）。
        assert_eq!(parse_custom_score(&serde_json::json!({ "score": "high" })), 0.0);
        assert_eq!(parse_custom_score(&serde_json::json!({})), 0.0);
        assert_eq!(parse_custom_score(&serde_json::json!({ "score": null })), 0.0);
    }

    #[test]
    fn detect_error_displays_provider_and_status() {
        let e = DetectError::Http { provider: "GPTZero", status: 401, body: "unauthorized".into() };
        assert!(format!("{e}").contains("GPTZero"));
        assert!(format!("{e}").contains("401"));
    }
}
