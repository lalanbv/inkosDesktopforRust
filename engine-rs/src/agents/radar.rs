//! 素材雷达（radar.ts + radar-source.ts，72 号）。
//!
//! 内置双源（番茄 JSON 榜单 API ×2 榜单 + 起点 HTML 榜页正则）→ 榜单文本化 →
//! 市场分析师提示词（逐字）→ LLM（温度 0.6）→ JSON 提取（首个 `{` 到末个 `}`）。
//! 网络源失败跳过（单源容错）；全空时提示词回落"基于知识分析"。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingEntry {
    pub title: String,
    pub author: String,
    pub category: String,
    pub extra: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRankings {
    pub platform: String,
    pub entries: Vec<RankingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadarRecommendation {
    #[serde(default)]
    pub platform: Value,
    #[serde(default)]
    pub genre: Value,
    #[serde(default)]
    pub concept: Value,
    #[serde(default)]
    pub confidence: Value,
    #[serde(default)]
    pub reasoning: Value,
    #[serde(default)]
    pub benchmark_titles: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadarResult {
    #[serde(default)]
    pub recommendations: Vec<Value>,
    #[serde(default)]
    pub market_summary: String,
    pub timestamp: String,
}

// ── 内置源 ──────────────────────────────────────────────────────

const FANQIE_RANK_TYPES: &[(u32, &str)] = &[(10, "热门榜"), (13, "黑马榜")];

/// 番茄小说：JSON 榜单 API（side_type 10 热门 / 13 黑马，各 15 条）。
/// 5s 预算客户端：榜单源慢/不可达时跳过（TS 单源容错语义 + 显式超时防挂起）。
fn radar_client() -> reqwest::Client {
    reqwest::Client::builder()
        // 禁系统代理探测（WPAD/PAC 可挂数十秒且不受请求超时覆盖）；外网源
        // 按直连 + 5s 总预算，超时即跳过该源。
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default()
}

async fn fetch_fanqie() -> PlatformRankings {
    let mut entries = Vec::new();
    let client = radar_client();
    for (side_type, label) in FANQIE_RANK_TYPES {
        let url = format!(
            "https://api-lf.fanqiesdk.com/api/novel/channel/homepage/rank/rank_list/v2/?aid=13&limit=15&offset=0&side_type={side_type}"
        );
        let Ok(response) = client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (compatible; InkOS/0.1)")
            .send()
            .await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(data) = response.json::<Value>().await else {
            continue;
        };
        let Some(list) = data
            .get("data")
            .and_then(|d| d.get("result"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for item in list {
            let field = |name: &str| {
                item.get(name)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            entries.push(RankingEntry {
                title: field("book_name"),
                author: field("author"),
                category: field("category"),
                extra: format!("[{label}]"),
            });
        }
    }
    PlatformRankings {
        platform: "番茄小说".to_string(),
        entries,
    }
}

/// 起点中文网：榜页 HTML 正则（`book.qidian.com/info/{id}` 链接文本，去重 ≤20）。
async fn fetch_qidian() -> PlatformRankings {
    let mut entries = Vec::new();
    let Ok(response) = radar_client()
        .get("https://www.qidian.com/rank/")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .send()
        .await
    else {
        return PlatformRankings {
            platform: "起点中文网".to_string(),
            entries,
        };
    };
    if !response.status().is_success() {
        return PlatformRankings {
            platform: "起点中文网".to_string(),
            entries,
        };
    }
    let Ok(html) = response.text().await else {
        return PlatformRankings {
            platform: "起点中文网".to_string(),
            entries,
        };
    };
    let re = regex::Regex::new(
        r#"<a[^>]*href="//book\.qidian\.com/info/\d+"[^>]*>([^<]+)</a>"#,
    )
    .unwrap();
    let mut seen = std::collections::HashSet::new();
    for caps in re.captures_iter(&html) {
        let title = caps.get(1).map(|m| m.as_str().trim()).unwrap_or_default();
        let title_chars = title.chars().count();
        if title.is_empty() || seen.contains(title) || title_chars <= 1 || title_chars >= 30 {
            continue;
        }
        seen.insert(title);
        entries.push(RankingEntry {
            title: title.to_string(),
            author: String::new(),
            category: String::new(),
            extra: "[起点热榜]".to_string(),
        });
        if entries.len() >= 20 {
            break;
        }
    }
    PlatformRankings {
        platform: "起点中文网".to_string(),
        entries,
    }
}

/// `formatRankingsForPrompt`：按平台分节 `- 标题 (作者) [分类] extra`；全空回退。
pub(crate) fn format_rankings_for_prompt(rankings: &[PlatformRankings]) -> String {
    let sections: Vec<String> = rankings
        .iter()
        .filter(|ranking| !ranking.entries.is_empty())
        .map(|ranking| {
            let lines: Vec<String> = ranking
                .entries
                .iter()
                .map(|entry| {
                    let author = if entry.author.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", entry.author)
                    };
                    let category = if entry.category.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", entry.category)
                    };
                    format!("- {}{}{} {}", entry.title, author, category, entry.extra)
                })
                .collect();
            format!("### {}\n{}", ranking.platform, lines.join("\n"))
        })
        .collect();
    if sections.is_empty() {
        "（未能获取到实时排行数据，请基于你的知识分析）".to_string()
    } else {
        sections.join("\n\n")
    }
}

/// `runRadar`：抓双源 → 分析师提示词 → LLM → JSON 解析。
pub async fn run_radar(router: &AgentRouter) -> Result<RadarResult, String> {
    let (fanqie, qidian) = tokio::join!(fetch_fanqie(), fetch_qidian());
    let rankings = vec![fanqie, qidian];
    let rankings_text = format_rankings_for_prompt(&rankings);

    let system_prompt = format!(
        r#"你是一个专业的网络小说市场分析师。下面是从各平台实时抓取的排行榜数据，请基于这些真实数据分析市场趋势。

## 实时排行榜数据

{rankings_text}

分析维度：
1. 从排行榜数据中识别当前热门题材和标签
2. 分析哪些类型的作品占据榜单高位
3. 发现市场空白和机会点（榜单上缺少但有潜力的方向）
4. 风险提示（榜单上过度扎堆的题材）

输出格式必须为 JSON：
{{
  "recommendations": [
    {{
      "platform": "平台名",
      "genre": "题材类型",
      "concept": "一句话概念描述",
      "confidence": 0.0-1.0,
      "reasoning": "推荐理由（引用具体榜单数据）",
      "benchmarkTitles": ["对标书1", "对标书2"]
    }}
  ],
  "marketSummary": "整体市场概述（基于真实榜单数据）"
}}

推荐数量：3-5个，按 confidence 降序排列。"#
    );

    let response = router
        .chat(
            "radar",
            vec![
                LLMMessage {
                    role: LLMRole::System,
                    content: system_prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
                LLMMessage {
                    role: LLMRole::User,
                    content: "请基于上面的实时排行榜数据，分析当前网文市场热度，给出开书建议。".to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            0.6,
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    parse_radar_result(&response.content)
}

/// `parseResult`：`/\{[\s\S]*\}/`（贪婪）→ 首个 `{` 到末个 `}` 子串解析。
pub fn parse_radar_result(content: &str) -> Result<RadarResult, String> {
    let start = content.find('{').ok_or("Radar output format error: no JSON found")?;
    let end = content
        .rfind('}')
        .ok_or("Radar output format error: no JSON found")?;
    if end <= start {
        return Err("Radar output format error: no JSON found".to_string());
    }
    let parsed: Value = serde_json::from_str(&content[start..=end])
        .map_err(|e| format!("Radar JSON parse error: {e}"))?;
    Ok(RadarResult {
        recommendations: parsed
            .get("recommendations")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        market_summary: parsed
            .get("marketSummary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        timestamp: crate::utils::utc_time::utc_now_iso(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_json_with_defaults() {
        let result = parse_radar_result(
            "前置噪音\n{\"recommendations\":[{\"platform\":\"番茄小说\",\"genre\":\"都市\",\"concept\":\"X\",\"confidence\":0.8}],\"marketSummary\":\"热度向上\"}\n尾",
        )
        .unwrap();
        assert_eq!(result.recommendations.len(), 1);
        assert_eq!(result.market_summary, "热度向上");
        assert!(!result.timestamp.is_empty());

        // 缺省字段回填。
        let result = parse_radar_result("{}").unwrap();
        assert!(result.recommendations.is_empty());
        assert_eq!(result.market_summary, "");

        assert!(parse_radar_result("no json").is_err());
    }

    #[test]
    fn rankings_format_and_fallback() {
        let rankings = vec![
            PlatformRankings {
                platform: "番茄小说".to_string(),
                entries: vec![RankingEntry {
                    title: "书A".to_string(),
                    author: "作者X".to_string(),
                    category: "都市".to_string(),
                    extra: "[热门榜]".to_string(),
                }],
            },
            PlatformRankings {
                platform: "空源".to_string(),
                entries: Vec::new(),
            },
        ];
        let text = format_rankings_for_prompt(&rankings);
        assert!(text.contains("### 番茄小说"), "{text}");
        assert!(text.contains("- 书A (作者X) [都市] [热门榜]"), "{text}");
        assert!(!text.contains("空源"), "{text}");

        let fallback = format_rankings_for_prompt(&[]);
        assert_eq!(fallback, "（未能获取到实时排行数据，请基于你的知识分析）");
    }
}
