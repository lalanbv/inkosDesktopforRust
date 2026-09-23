//! research_web 聊天工具（85 号）。
//!
//! 移植自 `packages/core/src/agent/agent-tools.ts` 的 `createResearchWebTool`、
//! `slugResearchTopic` 与 `readResearchSearchConfig`。读 inkos.json 的
//! researchSearch 配置，走确定性研究链（见 [`crate::agents::researcher`]`），
//! 报告写入 `.inkos/research/` 且只是参考资料，不改正典。

use std::path::Path;

use serde_json::{json, Value};

use crate::agents::researcher::{
    run_research_report, ResearchInput, ResearchTransport,
};
use crate::interaction::project_tools::{error_result, ToolResult};
use crate::interaction::registry::{schema_description, schema_parameters, MutationKind, ToolDef};
use crate::utils::utc_time::utc_now_iso;

/// `ResearchSearchConfigSchema`（inkos.json researchSearch 节）。
#[derive(Debug, Clone, Default)]
pub struct ResearchSearchConfig {
    pub enabled: bool,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
}

/// `readResearchSearchConfig`：inkos.json 缺失/损坏/节缺失 → 默认（禁用）。
pub async fn read_research_search_config(project_root: &Path) -> ResearchSearchConfig {
    let Ok(raw) = tokio::fs::read_to_string(project_root.join("inkos.json")).await else {
        return ResearchSearchConfig::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return ResearchSearchConfig::default();
    };
    let Some(node) = value.get("researchSearch").and_then(Value::as_object) else {
        return ResearchSearchConfig::default();
    };
    let str_field = |key: &str| {
        node.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    ResearchSearchConfig {
        enabled: node.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        base_url: str_field("baseUrl"),
        api_key: str_field("apiKey"),
        api_key_env: str_field("apiKeyEnv"),
    }
}

/// 生产传输：Tavily（配置 enabled 时带凭据；禁用时无 key → 每查询失败进
/// partialFailures，报告仍产出——TS 语义：禁用 ≠ 不调）。
pub struct TavilyTransport {
    options: crate::utils::web_search::WebSearchOptions,
}

impl TavilyTransport {
    pub fn from_config(config: &ResearchSearchConfig) -> Self {
        let options = if config.enabled {
            crate::utils::web_search::WebSearchOptions {
                api_key: config.api_key.clone(),
                api_key_env: config.api_key_env.clone(),
                base_url: config.base_url.clone(),
            }
        } else {
            crate::utils::web_search::WebSearchOptions::default()
        };
        Self { options }
    }
}

#[async_trait::async_trait]
impl ResearchTransport for TavilyTransport {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<crate::utils::web_search::SearchResult>, String> {
        crate::utils::web_search::search_web(query, max_results, &self.options).await
    }
    async fn fetch(&self, url: &str, max_chars: usize) -> Result<String, String> {
        crate::utils::web_search::fetch_url(url, max_chars).await
    }
}

fn text_result(text: impl Into<String>, details: Option<Value>) -> ToolResult {
    ToolResult { text: text.into(), details, is_error: false }
}

/// `slugResearchTopic`：NFKC lower + 非 [a-z0-9 汉字] 折叠 `-` + 首尾剥 +
/// 60 码元 + 空回退 "research"。
pub fn slug_research_topic(topic: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = topic.nfkc().collect::<String>().to_lowercase();
    static NON_ALNUM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = NON_ALNUM.get_or_init(|| regex::Regex::new(r"[^a-z0-9\u{4e00}-\u{9fff}]+").unwrap());
    let slugified = re.replace_all(normalized.trim(), "-");
    let trimmed = slugified.trim_matches('-').to_string();
    let mut units = 0usize;
    let mut out = String::new();
    for ch in trimmed.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 60 {
            break;
        }
        units += ch_units;
        out.push(ch);
    }
    if out.is_empty() { "research".to_string() } else { out }
}

/// `research_web` 执行器。
pub async fn tool_research_web(
    project_root: &Path,
    transport: &dyn ResearchTransport,
    args: &Value,
) -> ToolResult {
    let Some(topic) = args
        .get("topic")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return error_result("research_web requires a topic argument");
    };
    let purpose = args.get("purpose").and_then(Value::as_str).unwrap_or_default();
    let static_purpose: &'static str = match purpose {
        "worldbuilding" | "era" | "profession" | "market" | "fact-check" | "general" => {
            // 枚举已核，转 'static（常量集合内）。
            match purpose {
                "worldbuilding" => "worldbuilding",
                "era" => "era",
                "profession" => "profession",
                "market" => "market",
                "fact-check" => "fact-check",
                _ => "general",
            }
        }
        _ => {
            return error_result(format!(
                "Invalid research_web.purpose: {purpose}（expected worldbuilding | era | profession | market | fact-check | general）"
            ))
        }
    };
    let depth = args.get("depth").and_then(Value::as_str).unwrap_or("standard");
    let static_depth: &'static str = match depth {
        "quick" | "standard" | "deep" => match depth {
            "quick" => "quick",
            "deep" => "deep",
            _ => "standard",
        },
        _ => {
            return error_result(format!(
                "Invalid research_web.depth: {depth}（expected quick | standard | deep）"
            ))
        }
    };
    let input = ResearchInput { topic: topic.to_string(), purpose: static_purpose, depth: static_depth };
    let report = match run_research_report(&input, transport).await {
        Ok(report) => report,
        Err(message) => return error_result(message),
    };
    let research_dir = project_root.join(".inkos").join("research");
    if let Err(e) = tokio::fs::create_dir_all(&research_dir).await {
        return error_result(e.to_string());
    }
    let file_name = format!(
        "{}-{}.md",
        utc_now_iso().replace([':', '.'], "-"),
        slug_research_topic(topic)
    );
    let report_path = research_dir.join(&file_name);
    if let Err(e) = tokio::fs::write(&report_path, &report.markdown).await {
        return error_result(e.to_string());
    }
    let report_path_text = report_path.to_string_lossy().to_string();
    let failures_line = if report.partial_failures.is_empty() {
        "Partial failures: none.".to_string()
    } else {
        format!("Partial failures: {}.", report.partial_failures.len())
    };
    text_result(
        [
            format!("Research report saved: {report_path_text}"),
            format!("Sources: {}; confidence: {}.", report.sources.len(), report.confidence),
            failures_line,
        ]
        .join("\n"),
        Some(json!({
            "kind": "research_report",
            "reportPath": report_path_text,
            "topic": topic,
            "purpose": static_purpose,
            "depth": static_depth,
            "sources": report.sources,
            "claims": report.claims,
            "confidence": report.confidence,
            "partialFailures": report.partial_failures,
        })),
    )
}

/// `research_web` schema（ResearchWebParams 逐字）。
pub fn research_tool_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "research_web",
            "description": "Collect traceable web research for worldbuilding, era, profession, market, or fact-check questions. Saves a Markdown report under .inkos/research/. It is reference material only; it must not modify books, chapters, or truth files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Research question or topic, e.g. 1990s county cold-storage accounting workflow or Tang dynasty courier stations.",
                    },
                    "purpose": {
                        "type": "string",
                        "enum": ["worldbuilding", "era", "profession", "market", "fact-check", "general"],
                        "description": "Why this research is needed. Research reports are references only and must not directly mutate story state.",
                    },
                    "depth": {
                        "type": "string",
                        "enum": ["quick", "standard", "deep"],
                        "description": "Research depth. Default standard.",
                    },
                },
                "required": ["topic", "purpose"],
            },
        },
    })
}


// ── 注册模块（R38b）：research_web 单件——写 .inkos/research/ 报告 →
//    ProjectWrite（TS 剔除名单不含）；available = research 分支在场。 ──

crate::interaction::registry::tool_def!(
    ResearchWeb,
    "research_web",
    MutationKind::ProjectWrite,
    ctx, args,
    { schema_description(&[research_tool_schema()], "research_web") },
    schema_parameters(&[research_tool_schema()], "research_web"),
    ctx.research_enabled,
    { {
        let config = read_research_search_config(ctx.root).await;
        let transport = TavilyTransport::from_config(&config);
        tool_research_web(ctx.root, &transport, args).await
    } }
);

/// 注册表汇聚口（registry 装配序 = 原 ChatToolRouter 分发链序）。
pub(crate) fn defs() -> Vec<Box<dyn ToolDef>> {
    vec![Box::new(ResearchWeb)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::researcher::ResearchTransport;
    use crate::utils::web_search::SearchResult;

    /// 无搜索能力传输（search 恒败——对齐未配置环境）。
    struct NoopTransport;

    #[async_trait::async_trait]
    impl ResearchTransport for NoopTransport {
        async fn search(&self, _query: &str, _max_results: usize) -> Result<Vec<SearchResult>, String> {
            Err("TAVILY_API_KEY not set.".to_string())
        }
        async fn fetch(&self, _url: &str, _max_chars: usize) -> Result<String, String> {
            Ok(String::new())
        }
    }

    #[test]
    fn slug_topic() {
        assert_eq!(slug_research_topic("1990 年代 县冷库会计"), "1990-年代-县冷库会计");
        assert_eq!(slug_research_topic("!!!"), "research");
        assert_eq!(slug_research_topic(&"x".repeat(100)).chars().count(), 60);
    }

    #[tokio::test]
    async fn research_tool_writes_report_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 无搜索能力（Noop：search 恒败）→ 报告仍产出（0 源 low）。
        let result = tool_research_web(
            root,
            &NoopTransport,
            &json!({ "topic": "1990 年代冷库会计", "purpose": "era", "depth": "quick" }),
        )
        .await;
        assert!(!result.is_error, "{}", result.text);
        let lines: Vec<&str> = result.text.split('\n').collect();
        assert!(lines[0].starts_with("Research report saved: "), "{}", result.text);
        assert!(lines[0].ends_with(".md"));
        assert_eq!(lines[1], "Sources: 0; confidence: low.");
        assert_eq!(lines[2], "Partial failures: 1.");
        let details = result.details.unwrap();
        assert_eq!(details["kind"], "research_report");
        assert_eq!(details["purpose"], "era");
        assert_eq!(details["depth"], "quick");
        // 报告落盘。
        let research_dir = root.join(".inkos").join("research");
        let entries: Vec<_> = std::fs::read_dir(&research_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let markdown = std::fs::read_to_string(entries[0].as_ref().unwrap().path()).unwrap();
        assert!(markdown.starts_with("# Research: 1990 年代冷库会计"), "{markdown}");
        // 校验错误。
        let bad_purpose = tool_research_web(root, &NoopTransport, &json!({ "topic": "t", "purpose": "nope" })).await;
        assert!(bad_purpose.is_error && bad_purpose.text.contains("Invalid research_web.purpose"));
        let no_topic = tool_research_web(root, &NoopTransport, &json!({ "purpose": "era" })).await;
        assert!(no_topic.is_error && no_topic.text.contains("requires a topic"));
    }
}
