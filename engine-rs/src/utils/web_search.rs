//! 联网搜索 + URL 抓取（85 号）。
//!
//! 移植自 `packages/core/src/utils/web-search.ts`：Tavily 搜索
//! （`searchWeb`，Bearer + body api_key 形态）与 URL 文本抓取
//! （`fetchUrl`，HTML 剥离 + 空白折叠 + UTF-16 截断）。

use serde_json::json;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Default)]
pub struct WebSearchOptions {
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    pub base_url: Option<String>,
}

/// `searchWeb`：Tavily API 搜索。无 key → Err（调用方捕获降级，不炸轮次）。
pub async fn search_web(
    query: &str,
    max_results: usize,
    options: &WebSearchOptions,
) -> Result<Vec<SearchResult>, String> {
    let env_name = options.api_key_env.clone().unwrap_or_else(|| "TAVILY_API_KEY".to_string());
    let api_key = options
        .api_key
        .clone()
        .or_else(|| std::env::var(&env_name).ok().filter(|v| !v.is_empty()))
        .ok_or_else(|| {
            format!(
                "{env_name} not set. Configure Studio research search or set the env var to enable web search."
            )
        })?;
    let endpoint = options
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("https://api.tavily.com/search")
        .to_string();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&json!({
            "api_key": api_key,
            "query": query,
            "max_results": max_results,
            "search_depth": "basic",
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Tavily search failed: {} {}", status.as_u16(), body));
    }
    let data: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(data
        .get("results")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| SearchResult {
                    title: item.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    url: item.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    snippet: item.get("content").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// `fetchUrl`：抓 URL 文本。HTML → script/style/标签剥离 + 空白折叠；
/// 截断按 UTF-16 码元（TS `slice`）。
pub async fn fetch_url(url: &str, max_chars: usize) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .header("Accept", "text/html, application/json, text/plain")
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Fetch failed: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("")
        ));
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().await.map_err(|e| format!("Fetch failed: {e}"))?;
    if content_type.contains("html") {
        static SCRIPT_STYLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static TAG: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        static BLANKS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let script_style = SCRIPT_STYLE
            .get_or_init(|| regex::Regex::new(r"(?i)<script[\s\S]*?</script>|<style[\s\S]*?</style>").unwrap());
        let tag = TAG.get_or_init(|| regex::Regex::new(r"<[^>]*>").unwrap());
        let blanks = BLANKS.get_or_init(|| regex::Regex::new(r"\s+").unwrap());
        let stripped = script_style.replace_all(&text, "");
        let stripped = tag.replace_all(&stripped, " ");
        let collapsed = blanks.replace_all(&stripped, " ").trim().to_string();
        return Ok(truncate_utf16(&collapsed, max_chars));
    }
    Ok(truncate_utf16(text.trim_start(), max_chars))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_error_message() {
        let options = WebSearchOptions::default();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(search_web("query", 3, &options))
            .unwrap_err();
        // 未设 TAVILY_API_KEY 的环境（CI 常态）→ 固定文案。
        if std::env::var("TAVILY_API_KEY").is_err() {
            assert!(error.starts_with("TAVILY_API_KEY not set."), "{error}");
        }
    }

    #[test]
    fn utf16_truncation() {
        assert_eq!(truncate_utf16("abcdef", 3), "abc");
        assert_eq!(truncate_utf16("雪夜古宅", 2), "雪夜");
        assert_eq!(truncate_utf16("", 5), "");
    }
}
