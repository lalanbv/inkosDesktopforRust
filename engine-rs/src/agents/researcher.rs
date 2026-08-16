//! 研究员（85 号）：确定性资料收集链（**不调 LLM**）。
//!
//! 移植自 `packages/core/src/agents/researcher.ts`：按 depth 生成查询 →
//! 逐查询搜索（URL 去重）→ 逐源抓取摘录 → 确定性 claims/confidence/
//! creative implications → Markdown 报告。搜索/抓取经 [`ResearchTransport`]
//! 注入（生产 Tavily；测试替身）。

use serde::{Deserialize, Serialize};

use crate::utils::web_search::SearchResult;

pub type ResearchPurpose = &'static str;
pub const PURPOSES: &[&str] = &["worldbuilding", "era", "profession", "market", "fact-check", "general"];

#[derive(Debug, Clone)]
pub struct ResearchInput {
    pub topic: String,
    pub purpose: &'static str,
    pub depth: &'static str,
}

/// 搜索/抓取传输抽象（Tavily 生产实现 + 测试替身）。
#[async_trait::async_trait]
pub trait ResearchTransport: Send + Sync {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String>;
    async fn fetch(&self, url: &str, max_chars: usize) -> Result<String, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchSource {
    pub id: String,
    pub title: String,
    pub url: String,
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchClaim {
    pub text: String,
    pub source_ids: Vec<String>,
    pub confidence: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchReport {
    pub summary: String,
    pub claims: Vec<ResearchClaim>,
    pub conflicts: Vec<String>,
    pub unknowns: Vec<String>,
    pub creative_implications: Vec<String>,
    pub sources: Vec<ResearchSource>,
    pub confidence: &'static str,
    pub query_log: Vec<String>,
    pub partial_failures: Vec<String>,
    pub markdown: String,
}

fn purpose_hints(purpose: &str) -> &'static str {
    match purpose {
        "worldbuilding" => "世界观 背景 生活细节",
        "era" => "年代 背景 制度 物价 生活",
        "profession" => "职业 流程 术语 工作细节",
        "market" => "市场 趋势 受众 竞品",
        "fact-check" => "事实核查 来源",
        _ => "资料 参考",
    }
}

/// `runResearchReport`：确定性收集链。
pub async fn run_research_report(
    input: &ResearchInput,
    transport: &dyn ResearchTransport,
) -> Result<ResearchReport, String> {
    let topic = input.topic.trim();
    if topic.is_empty() {
        return Err("research topic is required.".to_string());
    }
    let (query_count, max_results, fetch_count) = match input.depth {
        "deep" => (3usize, 5usize, 6usize),
        "standard" => (2, 4, 4),
        _ => (1, 3, 2),
    };
    let queries = build_queries(topic, input.purpose, input.depth);
    let mut query_log: Vec<String> = Vec::new();
    let mut partial_failures: Vec<String> = Vec::new();
    let mut found: Vec<SearchResult> = Vec::new();
    let mut seen_urls: Vec<String> = Vec::new();

    for query in queries.iter().take(query_count) {
        query_log.push(query.clone());
        match transport.search(query, max_results).await {
            Ok(results) => {
                for result in results {
                    if result.url.is_empty() || seen_urls.contains(&result.url) {
                        continue;
                    }
                    seen_urls.push(result.url.clone());
                    found.push(result);
                }
            }
            Err(error) => {
                partial_failures.push(format!("search failed for \"{query}\": {error}"));
            }
        }
    }

    let mut sources: Vec<ResearchSource> = Vec::new();
    for result in found.iter().take(fetch_count) {
        let mut excerpt: Option<String> = None;
        match transport.fetch(&result.url, 1800).await {
            Ok(text) => {
                let sentences = first_sentences(&text, 3);
                if !sentences.is_empty() {
                    excerpt = Some(sentences);
                }
            }
            Err(error) => {
                partial_failures.push(format!("fetch failed for \"{}\": {error}", result.url));
            }
        }
        let id = format!("S{}", sources.len() + 1);
        sources.push(ResearchSource {
            id,
            title: if result.title.is_empty() { result.url.clone() } else { result.title.clone() },
            url: result.url.clone(),
            snippet: result.snippet.clone(),
            excerpt,
        });
    }

    let claims: Vec<ResearchClaim> = sources
        .iter()
        .map(|source| {
            let base = source
                .excerpt
                .as_deref()
                .filter(|text| !text.is_empty())
                .unwrap_or(if source.snippet.is_empty() { source.title.as_str() } else { source.snippet.as_str() });
            ResearchClaim {
                text: {
                    let first = first_sentences(base, 1);
                    if first.is_empty() { source.title.clone() } else { first }
                },
                source_ids: vec![source.id.clone()],
                confidence: if source.excerpt.is_some() { "medium" } else { "low" },
            }
        })
        .collect();
    let unknowns: Vec<String> = if sources.is_empty() {
        vec!["No usable sources were collected. Treat this report as incomplete.".to_string()]
    } else if !partial_failures.is_empty() {
        vec!["Some queries or source fetches failed; verify critical facts before using them as hard canon.".to_string()]
    } else {
        Vec::new()
    };
    let confidence = if sources.len() >= 3 && partial_failures.is_empty() {
        "high"
    } else if !sources.is_empty() {
        "medium"
    } else {
        "low"
    };
    let report = ResearchReport {
        summary: format!("Research collected {} source(s) for \"{topic}\" ({}, {}).", sources.len(), input.purpose, input.depth),
        claims,
        conflicts: Vec::new(),
        unknowns,
        creative_implications: build_creative_implications(input.purpose, sources.len()),
        sources,
        confidence,
        query_log,
        partial_failures,
        markdown: String::new(),
    };
    let markdown = render_research_markdown(topic, input, &report);
    Ok(ResearchReport { markdown, ..report })
}

fn build_queries(topic: &str, purpose: &str, depth: &str) -> Vec<String> {
    let hint = purpose_hints(purpose);
    let mut queries = vec![format!("{topic} {hint}")];
    if depth != "quick" {
        queries.push(format!("{topic} 资料 来源"));
    }
    if depth == "deep" {
        queries.push(format!("{topic} 争议 误区 核查"));
    }
    queries
}

/// `firstSentences`：按句末标点切句取前 N + 700 码元截断。
fn first_sentences(text: &str, max_sentences: usize) -> String {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return String::new();
    }
    static SENTENCE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = SENTENCE_RE
        .get_or_init(|| regex::Regex::new(r"[^。！？.!?]+[。！？.!?]?").unwrap());
    let joined: String = re
        .find_iter(&normalized)
        .take(max_sentences)
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string();
    let mut units = 0usize;
    let mut out = String::new();
    for ch in joined.chars() {
        let ch_units = ch.len_utf16();
        if units + ch_units > 700 {
            break;
        }
        units += ch_units;
        out.push(ch);
    }
    out
}

fn build_creative_implications(purpose: &str, source_count: usize) -> Vec<String> {
    if source_count == 0 {
        return vec!["Do not promote any collected item to hard story canon yet.".to_string()];
    }
    match purpose {
        "profession" => vec!["Use workflow details and terminology as texture, but keep hard claims source-backed.".to_string()],
        "era" => vec!["Use era constraints to police anachronisms in scenes, props, dialogue, and institutions.".to_string()],
        "worldbuilding" => vec!["Convert verified social, material, and institutional details into scene rules rather than exposition dumps.".to_string()],
        "market" => vec!["Treat market observations as positioning hints, not as story canon.".to_string()],
        "fact-check" => vec!["Facts with only one source should remain soft until cross-checked.".to_string()],
        _ => vec!["Use sourced details as references; unresolved points should stay out of hard canon.".to_string()],
    }
}

fn render_research_markdown(topic: &str, input: &ResearchInput, report: &ResearchReport) -> String {
    let mut lines: Vec<String> = vec![
        format!("# Research: {topic}"),
        String::new(),
        format!("- Purpose: {}", input.purpose),
        format!("- Depth: {}", input.depth),
        format!("- Confidence: {}", report.confidence),
        String::new(),
        "## Summary".to_string(),
        report.summary.clone(),
        String::new(),
        "## Claims".to_string(),
    ];
    if report.claims.is_empty() {
        lines.push("- No sourced claims collected.".to_string());
    } else {
        for claim in &report.claims {
            let ids = claim
                .source_ids
                .iter()
                .map(|id| format!("[{id}]"))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("- {} ({ids}, {})", claim.text, claim.confidence));
        }
    }
    lines.push(String::new());
    lines.push("## Conflicts".to_string());
    if report.conflicts.is_empty() {
        lines.push("- None detected by the collection pass.".to_string());
    } else {
        lines.extend(report.conflicts.iter().map(|item| format!("- {item}")));
    }
    lines.push(String::new());
    lines.push("## Unknowns".to_string());
    if report.unknowns.is_empty() {
        lines.push("- None recorded.".to_string());
    } else {
        lines.extend(report.unknowns.iter().map(|item| format!("- {item}")));
    }
    lines.push(String::new());
    lines.push("## Creative implications".to_string());
    lines.extend(report.creative_implications.iter().map(|item| format!("- {item}")));
    lines.push(String::new());
    lines.push("## Sources".to_string());
    if report.sources.is_empty() {
        lines.push("No sources collected.".to_string());
    } else {
        for source in &report.sources {
            lines.push(format!("### [{}] {}", source.id, source.title));
            lines.push(source.url.clone());
            lines.push(String::new());
            lines.push(
                source
                    .excerpt
                    .clone()
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| source.snippet.clone()),
            );
        }
    }
    lines.push(String::new());
    lines.push("## Query log".to_string());
    lines.extend(report.query_log.iter().map(|query| format!("- {query}")));
    lines.push(String::new());
    lines.push("## Partial failures".to_string());
    if report.partial_failures.is_empty() {
        lines.push("- None.".to_string());
    } else {
        lines.extend(report.partial_failures.iter().map(|item| format!("- {item}")));
    }
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeTransport {
        search_results: Vec<SearchResult>,
        search_error: Option<String>,
        fetch_text: String,
        fetch_error: Option<String>,
        search_calls: Mutex<Vec<(String, usize)>>,
    }

    #[async_trait::async_trait]
    impl ResearchTransport for FakeTransport {
        async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
            self.search_calls.lock().unwrap().push((query.to_string(), max_results));
            if let Some(error) = &self.search_error {
                return Err(error.clone());
            }
            Ok(self.search_results.clone())
        }
        async fn fetch(&self, _url: &str, _max_chars: usize) -> Result<String, String> {
            if let Some(error) = &self.fetch_error {
                return Err(error.clone());
            }
            Ok(self.fetch_text.clone())
        }
    }

    fn fake() -> FakeTransport {
        FakeTransport {
            search_results: vec![
                SearchResult { title: "冷库史料".to_string(), url: "https://a.example/1".to_string(), snippet: "1990 年代县冷库采用三联账。".to_string() },
                SearchResult { title: "第二篇".to_string(), url: "https://a.example/2".to_string(), snippet: "赔偿流程需要主任签字。".to_string() },
                SearchResult { title: "".to_string(), url: "https://a.example/3".to_string(), snippet: "无标题来源。".to_string() },
            ],
            search_error: None,
            fetch_text: "冷库夜班的账页有三联。第二句。第三句。第四句超出。".to_string(),
            fetch_error: None,
            search_calls: Mutex::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn standard_depth_collects_sources_and_renders_markdown() {
        let transport = fake();
        let input = ResearchInput {
            topic: "1990 年代县冷库会计流程".to_string(),
            purpose: "era",
            depth: "standard",
        };
        let report = run_research_report(&input, &transport).await.unwrap();
        // standard：2 查询 × 4 上限，抓 4（来源 3 去重后）。
        assert_eq!(report.query_log.len(), 2);
        assert!(report.query_log[0].contains("年代 背景 制度 物价 生活"), "{:?}", report.query_log);
        assert!(report.query_log[1].contains("资料 来源"));
        assert_eq!(report.sources.len(), 3);
        assert_eq!(report.sources[0].id, "S1");
        assert!(report.sources[2].title.contains("a.example/3"), "空标题回退 URL");
        // 摘录取前三句。
        assert!(!report.sources[0].excerpt.as_deref().unwrap().contains("第四句超出"));
        assert_eq!(report.claims.len(), 3);
        assert_eq!(report.claims[0].confidence, "medium");
        // 3 来源 + 无失败 → high。
        assert_eq!(report.confidence, "high");
        assert!(report.unknowns.is_empty());
        assert_eq!(
            report.creative_implications,
            vec!["Use era constraints to police anachronisms in scenes, props, dialogue, and institutions."]
        );
        // markdown 形态。
        assert!(report.markdown.starts_with("# Research: 1990 年代县冷库会计流程\n\n- Purpose: era\n- Depth: standard\n- Confidence: high"));
        assert!(report.markdown.contains("### [S1] 冷库史料\nhttps://a.example/1"));
        assert!(report.markdown.contains("## Partial failures\n- None."));
    }

    #[tokio::test]
    async fn deep_quick_queries_and_failures() {
        // quick：单查询。
        let transport = fake();
        let input = ResearchInput { topic: "t".to_string(), purpose: "general", depth: "quick" };
        let report = run_research_report(&input, &transport).await.unwrap();
        assert_eq!(report.query_log.len(), 1);
        assert_eq!(report.sources.len(), 2, "quick 只抓 2 源");
        assert_eq!(report.confidence, "medium", "2 源无失败");

        // 搜索恒败：0 源 + 失败记录 + low + unknowns。
        let failing = FakeTransport { search_error: Some("TAVILY_API_KEY not set.".to_string()), ..fake() };
        let input = ResearchInput { topic: "t".to_string(), purpose: "market", depth: "deep" };
        let report = run_research_report(&input, &failing).await.unwrap();
        assert_eq!(report.query_log.len(), 3, "deep 三查询");
        assert!(report.sources.is_empty());
        assert_eq!(report.confidence, "low");
        assert_eq!(report.partial_failures.len(), 3);
        assert!(report.partial_failures[0].contains("search failed for \"t 市场 趋势 受众 竞品\": TAVILY_API_KEY not set."));
        assert_eq!(report.unknowns, vec!["No usable sources were collected. Treat this report as incomplete."]);
        assert_eq!(
            report.creative_implications,
            vec!["Do not promote any collected item to hard story canon yet."]
        );
        assert!(report.markdown.contains("- No sourced claims collected."));
        assert!(report.markdown.contains("No sources collected."));
    }
}
