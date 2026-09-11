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

/// G14a/335 号"选后再析"：勾选范围。空数组/None = 该维度不过滤。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadarSelection {
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub titles: Vec<String>,
}

fn norm_set(items: &[String]) -> std::collections::HashSet<String> {
    items.iter().map(|x| x.to_lowercase()).collect()
}

/// 按勾选范围过滤榜单（纯函数）：三维度独立，空 = 不过滤该维度。
pub fn filter_rankings_by_selection(
    rankings: Vec<PlatformRankings>,
    selection: Option<&RadarSelection>,
) -> Vec<PlatformRankings> {
    let Some(selection) = selection else {
        return rankings;
    };
    let platforms = norm_set(&selection.platforms);
    let categories = norm_set(&selection.categories);
    let titles = norm_set(&selection.titles);
    if platforms.is_empty() && categories.is_empty() && titles.is_empty() {
        return rankings;
    }
    rankings
        .into_iter()
        .filter(|r| platforms.is_empty() || platforms.contains(&r.platform.to_lowercase()))
        .map(|mut r| {
            r.entries.retain(|e| {
                if !categories.is_empty()
                    && categories.iter().any(|cat| e.category.to_lowercase().contains(cat.as_str()))
                {
                    return true;
                }
                if !titles.is_empty()
                    && titles.iter().any(|t| e.title.to_lowercase().contains(t.as_str()))
                {
                    return true;
                }
                categories.is_empty() && titles.is_empty()
            });
            r
        })
        .filter(|r| !r.entries.is_empty())
        .collect()
}

/// 免费扫榜：只抓各源榜单，不调 LLM。
pub async fn fetch_rankings() -> Vec<PlatformRankings> {
    let (fanqie, qidian) = tokio::join!(fetch_fanqie(), fetch_qidian());
    vec![fanqie, qidian]
}

/// 对勾选范围做 LLM 分析（选后再析）：产出信号卡（crowding/differentiation）。
pub async fn run_radar_analyze(
    router: &AgentRouter,
    rankings: &[PlatformRankings],
    selection: Option<&RadarSelection>,
) -> Result<RadarResult, String> {
    let scoped = filter_rankings_by_selection(rankings.to_vec(), selection);
    let scoped_any = selection
        .map(|s| !s.platforms.is_empty() || !s.categories.is_empty() || !s.titles.is_empty())
        .unwrap_or(false);
    let scope_note = if scoped_any { "（本次分析仅针对用户勾选的范围，请严格基于以下数据）" } else { "" };
    let rankings_text = format!("{scope_note}
{}", format_rankings_for_prompt(&scoped))
        .trim()
        .to_string();

    let system_prompt = format!(
        r#"你是一个专业的网络小说市场分析师。下面是从各平台实时抓取的排行榜数据，请基于这些真实数据分析市场趋势。

## 实时排行榜数据

{rankings_text}

分析维度：
1. 从排行榜数据中识别当前热门题材和标签
2. 分析哪些类型的作品占据榜单高位
3. 发现市场空白和机会点（榜单上缺少但有潜力的方向）
4. 风险提示（榜单上过度扎堆的题材，给出拥挤度）

输出格式必须为 JSON：
{{
  "recommendations": [
    {{
      "platform": "平台名",
      "genre": "题材类型",
      "concept": "一句话概念描述",
      "confidence": 0.0-1.0,
      "reasoning": "推荐理由（引用具体榜单数据）",
      "benchmarkTitles": ["对标书1", "对标书2"],
      "crowding": "high|medium|low（该题材当前拥挤度）",
      "differentiation": "差异化机会一句话（如何避开扎堆）"
    }}
  ],
  "marketSummary": "整体市场概述（基于真实榜单数据）"
}}

推荐数量：3-5个，按 confidence 降序排列。"#
    );
    let _ = &system_prompt;

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

/// `runRadar`：抓双源 → 全量分析（daemon/tick 与旧调用方兼容入口）。
pub async fn run_radar(router: &AgentRouter) -> Result<RadarResult, String> {
    let rankings = fetch_rankings().await;
    run_radar_analyze(router, &rankings, None).await
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
    fn parse_passes_signal_card_fields_through() {
        // G14a/335 号：crowding/differentiation 信号卡字段透传（Vec<Value> 宽松）。
        let result = parse_radar_result(
            "{\"recommendations\":[{\"platform\":\"番茄\",\"genre\":\"都市\",\"concept\":\"X\",\"confidence\":0.9,\"crowding\":\"high\",\"differentiation\":\"避开同质化，主打悬疑元素\"}]}",
        )
        .unwrap();
        let rec = &result.recommendations[0];
        assert_eq!(rec["crowding"], "high");
        assert_eq!(rec["differentiation"], "避开同质化，主打悬疑元素");
    }

    #[test]
    fn selection_filters_platforms_categories_titles() {
        let rankings = vec![
            PlatformRankings {
                platform: "番茄小说".to_string(),
                entries: vec![
                    RankingEntry { title: "书A".into(), author: String::new(), category: "都市".into(), extra: String::new() },
                    RankingEntry { title: "书B".into(), author: String::new(), category: "仙侠".into(), extra: String::new() },
                ],
            },
            PlatformRankings {
                platform: "起点中文网".to_string(),
                entries: vec![
                    RankingEntry { title: "书C".into(), author: String::new(), category: "都市".into(), extra: String::new() },
                ],
            },
        ];

        // 空勾选 = 全量。
        assert_eq!(filter_rankings_by_selection(rankings.clone(), None).len(), 2);
        assert_eq!(
            filter_rankings_by_selection(rankings.clone(), Some(&RadarSelection::default())).len(),
            2
        );

        // 平台勾选：只留起点。
        let only_qidian = filter_rankings_by_selection(
            rankings.clone(),
            Some(&RadarSelection { platforms: vec!["起点中文网".into()], ..Default::default() }),
        );
        assert_eq!(only_qidian.len(), 1);
        assert_eq!(only_qidian[0].platform, "起点中文网");

        // 分类勾选：跨平台只留都市条目。
        let urban = filter_rankings_by_selection(
            rankings.clone(),
            Some(&RadarSelection { categories: vec!["都市".into()], ..Default::default() }),
        );
        assert_eq!(urban.len(), 2);
        assert_eq!(urban[0].entries.len(), 1);
        assert_eq!(urban[1].entries.len(), 1);

        // 无匹配条目的平台整源剔除。
        let missing = filter_rankings_by_selection(
            rankings,
            Some(&RadarSelection { categories: vec!["科幻".into()], ..Default::default() }),
        );
        assert!(missing.is_empty());
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
