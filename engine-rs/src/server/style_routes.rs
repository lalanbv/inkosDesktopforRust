//! 风格与导入域端点（55 号）：风格指纹 / LLM 文风向导 / 正典导入 / 番外正典读。
//!
//! 契约来源 `packages/studio/src/api/server.ts`：
//! - `POST /style/analyze`（L6184）：`analyzeStyle` 纯函数（默认 zh）→ 直接返回
//!   StyleProfile；text 空 → 400 `"text is required"`
//! - `POST /books/:id/style/import`（L6199）：`generateStyleGuide`——统计指纹
//!   （style_profile.json）+ LLM 定性拆解（≥500 字；否则/失败/空响应回退
//!   确定性指南）+ 写作方法论拼接 → style_guide.md；SSE style:*
//! - `POST /books/:id/import/canon`（L6240）：`importCanon`——父书 8 真相 →
//!   LLM 正典生成（temp 0.3）+ 确定性 meta 块 → parent_canon.md；父书章节
//!   样本 ≥500 字时顺带风格向导；SSE import:*
//! - `GET /books/:id/fanfic`（L6300）：读 `story/fanfic_canon.md`（缺 →
//!   content:null 仍 200）
//!
//! 暂缓件：`POST /books/:id/import/chapters`（依赖 architect 域基础设定
//! 生成）、`POST /fanfic/init` + `/fanfic/refresh`（canon 提取长链）、
//! `POST /spinoff/init`（依赖 createStatus + architect）。

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::agents::rules_reader::read_genre_profile;
use crate::agents::style_analyzer::analyze_style;
use crate::llm::agent_router::AgentRouter;
use crate::llm::provider::{LLMMessage, LLMRole};
use crate::models::style_profile::StyleProfile;
use crate::server::books_routes::BooksRuntime;
use crate::state::manager::StateManager;
use crate::utils::language::WritingLanguage;
use crate::utils::utc_time::utc_now_iso;
use crate::utils::writing_methodology::build_writing_methodology_section;

type ApiError = (StatusCode, Json<Value>);

fn flat_internal(message: impl std::fmt::Display) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": message.to_string() })),
    )
}

// ── POST /api/v1/style/analyze ───────────────────────────────────

pub async fn style_analyze(body: Bytes) -> impl IntoResponse {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let Some(text) = parsed.get("text").and_then(Value::as_str).map(str::trim) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "text is required" })));
    };
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "text is required" })));
    }
    // TS analyzeStyle(text, sourceName ?? "unknown")：默认 zh。
    let source_name = parsed.get("sourceName").and_then(Value::as_str).unwrap_or("unknown");
    let profile = analyze_style(text, Some(source_name), WritingLanguage::Zh, Some(&utc_now_iso()));
    (StatusCode::OK, Json(serde_json::to_value(&profile).unwrap_or_default()))
}

// ── 风格向导核心（generateStyleGuide） ───────────────────────────

/// 确定性风格指南（双语模板逐字；LLM 不可用/空响应/短样本兜底）。
fn build_deterministic_style_guide(profile: &StyleProfile, language: WritingLanguage, reason: &str) -> String {
    if language == WritingLanguage::En {
        [
            "# Style Guide".to_string(),
            String::new(),
            format!("> {reason}"),
            String::new(),
            "## Statistical Fingerprint".to_string(),
            format!("- Source: {}", profile.source_name.as_deref().unwrap_or("unknown")),
            format!("- Average sentence length: {}", profile.avg_sentence_length),
            format!("- Sentence length variance: {}", profile.sentence_length_std_dev),
            format!("- Average paragraph length: {}", profile.avg_paragraph_length),
            format!("- Vocabulary diversity: {}%", (profile.vocabulary_diversity * 100.0).round() as i64),
            if profile.top_patterns.is_empty() {
                "- Repeated openings: none obvious in this sample".to_string()
            } else {
                format!("- Repeated openings: {}", profile.top_patterns.join(", "))
            },
            if profile.rhetorical_features.is_empty() {
                "- Rhetorical features: none obvious in this sample".to_string()
            } else {
                format!("- Rhetorical features: {}", profile.rhetorical_features.join(", "))
            },
            String::new(),
            "## How To Use".to_string(),
            "- Treat this as a lightweight style fingerprint, not a full imitation bible.".to_string(),
            "- Keep sentence and paragraph rhythm close to the sample when drafting.".to_string(),
            "- If this guide feels too thin, import a longer excerpt later; the file will be replaced.".to_string(),
        ]
        .join("\n")
    } else {
        [
            "# 文风指南".to_string(),
            String::new(),
            format!("> {reason}"),
            String::new(),
            "## 统计风格指纹".to_string(),
            format!("- 来源：{}", profile.source_name.as_deref().unwrap_or("unknown")),
            format!("- 平均句长：{}", profile.avg_sentence_length),
            format!("- 句长波动：{}", profile.sentence_length_std_dev),
            format!("- 平均段落长度：{}", profile.avg_paragraph_length),
            format!("- 词汇多样性：{}%", (profile.vocabulary_diversity * 100.0).round() as i64),
            if profile.top_patterns.is_empty() {
                "- 高频句首/模式：样本内不明显".to_string()
            } else {
                format!("- 高频句首/模式：{}", profile.top_patterns.join("、"))
            },
            if profile.rhetorical_features.is_empty() {
                "- 修辞特征：样本内不明显".to_string()
            } else {
                format!("- 修辞特征：{}", profile.rhetorical_features.join("、"))
            },
            String::new(),
            "## 使用方式".to_string(),
            "- 这是一份轻量文风指纹，不是完整仿写圣经。".to_string(),
            "- 后续写作优先参考句长、段落长度、节奏波动和可见修辞。".to_string(),
            "- 如果想得到更稳定的定性拆解，后续可以导入更长片段覆盖本文件。".to_string(),
        ]
        .join("\n")
    }
}

/// LLM 定性拆解的系统提示（双语逐字）。
fn style_system_prompt(language: WritingLanguage) -> &'static str {
    if language == WritingLanguage::En {
        "You are a literary style analyst. Analyze the writing style of the reference text and extract qualitative, imitable features.\n\nOutput format (Markdown):\n## Narrative Voice & Tone\n(detached / fervent / ironic / warm / ..., with 1-2 quoted lines from the text)\n\n## Dialogue Style\n(shared traits in how characters speak: sentence length, verbal tics, dialect markers, dialogue rhythm)\n\n## Scene Description\n(sensory preferences, choice of imagery, description density, how setting ties to emotion)\n\n## Transitions & Connective Technique\n(how scenes switch, how time jumps are handled, paragraph-to-paragraph transitions)\n\n## Pacing\n(distribution of long vs short sentences, paragraph-length preference, how climaxes and lulls alternate)\n\n## Diction\n(signature high-frequency word choices, figurative/rhetorical tendencies, degree of colloquialism)\n\n## Emotional Expression\n(direct lyricism vs externalized action, frequency and style of interior monologue)\n\n## Distinctive Habits\n(any personal writing habits worth imitating)\n\nBase the analysis on the text's actual features, not generalities. Support each section with 1-2 quoted lines from the original."
    } else {
        "你是一位文学风格分析专家。分析参考文本的写作风格，提取可供模仿的定性特征。\n\n输出格式（Markdown）：\n## 叙事声音与语气\n（冷峻/热烈/讽刺/温情/...，附1-2个原文例句）\n\n## 对话风格\n（角色说话的共性特征：句子长短、口头禅倾向、方言痕迹、对话节奏）\n\n## 场景描写特征\n（五感偏好、意象选择、描写密度、环境与情绪的关联方式）\n\n## 转折与衔接手法\n（场景如何切换、时间跳跃的处理方式、段落间的过渡特征）\n\n## 节奏特征\n（长短句分布、段落长度偏好、高潮/舒缓的交替方式）\n\n## 词汇偏好\n（高频特色用词、比喻/修辞倾向、口语化程度）\n\n## 情绪表达方式\n（直白抒情 vs 动作外化、内心独白的频率和风格）\n\n## 独特习惯\n（任何值得模仿的个人写作习惯）\n\n分析必须基于原文实际特征，不要泛泛而谈。每个部分用1-2个原文例句佐证。"
    }
}

/// 风格向导主链（TS `generateStyleGuide`）：指纹落盘 →（≥500 字）LLM 定性
/// 拆解（失败/空回退确定性指南）→ 拼接写作方法论 → style_guide.md。
async fn generate_style_guide(
    state: &StateManager,
    router: &AgentRouter,
    builtin_genres_dir: &std::path::Path,
    book_id: &str,
    sample: &str,
    source_name: Option<&str>,
) -> Result<String, String> {
    if sample.is_empty() {
        return Err("Reference text is required for style extraction.".to_string());
    }
    let book_dir = state.book_dir(book_id);
    let story_dir = book_dir.join("story");
    tokio::fs::create_dir_all(&story_dir)
        .await
        .map_err(|e| e.to_string())?;
    let book = state.load_book_config(book_id).await.map_err(|e| e.to_string())?;
    let parsed_genre = read_genre_profile(state.project_root(), &book.genre, builtin_genres_dir)
        .await
        .map_err(|e| e.to_string())?;
    let language = if book.language.as_deref().unwrap_or(&parsed_genre.profile.language) == "en" {
        WritingLanguage::En
    } else {
        WritingLanguage::Zh
    };

    let profile = analyze_style(sample, source_name, language, Some(&utc_now_iso()));
    let profile_json = serde_json::to_string_pretty(&profile)
        .map_err(|e| e.to_string())?;
    tokio::fs::write(story_dir.join("style_profile.json"), profile_json)
        .await
        .map_err(|e| e.to_string())?;

    // TS sample.length：UTF-16 码元计数。
    let sample_len = sample.encode_utf16().count();
    let qualitative_guide = if sample_len < 500 {
        let reason = if language == WritingLanguage::En {
            format!("The sample is short ({sample_len} chars), so this guide uses the statistical fingerprint instead of LLM qualitative extraction.")
        } else {
            format!("样本文本较短（{sample_len}字），本次先使用统计指纹生成文风指南，不强行调用 LLM 做定性拆解。")
        };
        build_deterministic_style_guide(&profile, language, &reason)
    } else {
        let user_prompt = if language == WritingLanguage::En {
            format!("Analyze the writing style of the following reference text:\n\n{sample}")
        } else {
            format!("分析以下参考文本的写作风格：\n\n{sample}")
        };
        match router
            .chat(
                "style-guide",
                vec![
                    LLMMessage { role: LLMRole::System, content: style_system_prompt(language).to_string(), tool_calls: None, tool_call_id: None },
                    LLMMessage { role: LLMRole::User, content: user_prompt, tool_calls: None, tool_call_id: None },
                ],
                0.3,
                None,
            )
            .await
        {
            Ok(outcome) if !outcome.content.trim().is_empty() => outcome.content,
            Ok(_) => build_deterministic_style_guide(
                &profile,
                language,
                if language == WritingLanguage::En {
                    "The LLM returned empty style analysis; using the statistical fingerprint fallback."
                } else {
                    "LLM 未返回有效文风分析，本次使用统计指纹兜底生成文风指南。"
                },
            ),
            Err(error) => {
                let reason = if language == WritingLanguage::En {
                    format!("LLM qualitative extraction failed: {error}. Using the statistical fingerprint fallback.")
                } else {
                    format!("LLM 定性拆解失败：{error}。本次使用统计指纹兜底生成文风指南。")
                };
                build_deterministic_style_guide(&profile, language, &reason)
            }
        }
    };

    let methodology = build_writing_methodology_section(language);
    let full_style_guide = format!("{qualitative_guide}\n\n{methodology}");
    tokio::fs::write(story_dir.join("style_guide.md"), &full_style_guide)
        .await
        .map_err(|e| e.to_string())?;
    Ok(full_style_guide)
}

/// 供创建/导入链复用的风格向导入口（58 号）。
pub async fn generate_style_guide_for_book(
    state: &StateManager,
    router: &AgentRouter,
    builtin_genres_dir: &std::path::Path,
    book_id: &str,
    sample: &str,
    source_name: Option<&str>,
) -> Result<String, String> {
    generate_style_guide(state, router, builtin_genres_dir, book_id, sample, source_name).await
}

// ── POST /api/v1/books/:id/style/import ──────────────────────────

pub async fn style_import(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let Some(text) = parsed.get("text").and_then(Value::as_str).map(str::trim) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "text is required" })));
    };
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "text is required" })));
    }
    let source_name = parsed.get("sourceName").and_then(Value::as_str).unwrap_or("unknown");
    runtime.hub.broadcast("style:start", &json!({ "bookId": book_id }));
    match generate_style_guide(
        &runtime.state,
        &*runtime.effective_router().await,
        &runtime.builtin_genres_dir,
        &book_id,
        text,
        Some(source_name),
    )
    .await
    {
        Ok(result) => {
            runtime.hub.broadcast("style:complete", &json!({ "bookId": book_id }));
            (StatusCode::OK, Json(json!({ "ok": true, "result": result })))
        }
        Err(error) => {
            runtime.hub.broadcast("style:error", &json!({ "bookId": book_id, "error": error }));
            flat_internal(error)
        }
    }
}

// ── POST /api/v1/books/:id/import/canon ──────────────────────────

/// 读文件，任何失败 → "(无)"。对齐 TS `readSafe`。
async fn read_safe(path: &std::path::Path) -> String {
    tokio::fs::read_to_string(path).await.unwrap_or_else(|_| "(无)".to_string())
}

/// 父书章节样本：.md 字典序前 5 个，累计 <20000（UTF-16 码元），
/// `\n\n---\n\n` 连接。对齐 TS `readParentChapterSample`。
async fn read_parent_chapter_sample(chapters_dir: &std::path::Path) -> String {
    let Ok(mut entries) = tokio::fs::read_dir(chapters_dir).await else {
        return String::new();
    };
    let mut files: Vec<String> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") {
            files.push(name);
        }
    }
    files.sort();
    let mut chunks: Vec<String> = Vec::new();
    let mut total_length = 0usize;
    for file in files.into_iter().take(5) {
        if total_length >= 20000 {
            break;
        }
        if let Ok(content) = tokio::fs::read_to_string(chapters_dir.join(&file)).await {
            total_length += content.encode_utf16().count();
            chunks.push(content);
        }
    }
    chunks.join("\n\n---\n\n")
}

/// 正典导入链（TS `importCanon`）。返回写入的 canon 全文（59 号 spinoff 复用）。
pub async fn import_canon(
    state: &StateManager,
    router: &AgentRouter,
    builtin_genres_dir: &std::path::Path,
    target_book_id: &str,
    parent_book_id: &str,
) -> Result<String, String> {
    let book_ids = state.list_books().await;
    let available = if book_ids.is_empty() {
        "(none)".to_string()
    } else {
        book_ids.join(", ")
    };
    if !book_ids.iter().any(|id| id == parent_book_id) {
        return Err(format!(
            "Parent book \"{parent_book_id}\" not found. Available: {available}"
        ));
    }
    if !book_ids.iter().any(|id| id == target_book_id) {
        return Err(format!(
            "Target book \"{target_book_id}\" not found. Available: {available}"
        ));
    }

    let parent_dir = state.book_dir(parent_book_id);
    let target_dir = state.book_dir(target_book_id);
    let story_dir = target_dir.join("story");
    tokio::fs::create_dir_all(&story_dir)
        .await
        .map_err(|e| e.to_string())?;
    let parent_book = state
        .load_book_config(parent_book_id)
        .await
        .map_err(|e| e.to_string())?;

    // Phase 5：story_frame 优先，空则回退 legacy story_bible。
    async fn read_parent_outline(parent_dir: &std::path::Path, new_rel: &str, legacy_rel: &str) -> String {
        let preferred = read_safe(&parent_dir.join("story").join(new_rel)).await;
        if !preferred.trim().is_empty() && preferred != "(无)" {
            return preferred;
        }
        read_safe(&parent_dir.join("story").join(legacy_rel)).await
    }
    let story_bible = read_parent_outline(&parent_dir, "outline/story_frame.md", "story_bible.md").await;
    let current_state = read_safe(&parent_dir.join("story").join("current_state.md")).await;
    let ledger = read_safe(&parent_dir.join("story").join("particle_ledger.md")).await;
    let hooks = read_safe(&parent_dir.join("story").join("pending_hooks.md")).await;
    let summaries = read_safe(&parent_dir.join("story").join("chapter_summaries.md")).await;
    let subplots = read_safe(&parent_dir.join("story").join("subplot_board.md")).await;
    let emotions = read_safe(&parent_dir.join("story").join("emotional_arcs.md")).await;
    let matrix = read_safe(&parent_dir.join("story").join("character_matrix.md")).await;

    let system = "你是一位网络小说架构师。基于正传的全部设定和状态文件，生成一份完整的\"正传正典参照\"文档，供番外写作和审计使用。\n\n输出格式（Markdown）：\n# 正传正典（《{正传书名}》）\n\n## 世界规则（完整，来自正传设定）\n（力量体系、地理设定、阵营关系、核心规则——完整复制，不压缩）\n\n## 正典约束（不可违反的事实）\n| 约束ID | 类型 | 约束内容 | 严重性 |\n|---|---|---|---|\n| C01 | 人物存亡 | ... | critical |\n（列出所有硬性约束：谁活着、谁死了、什么事件已经发生、什么规则不可违反）\n\n## 角色快照\n| 角色 | 当前状态 | 性格底色 | 对话特征 | 已知信息 | 未知信息 |\n|---|---|---|---|---|---|\n（从状态卡和角色矩阵中提取每个重要角色的完整快照）\n\n## 角色双态处理原则\n- 未来会变强的角色：写潜力暗示\n- 未来会黑化的角色：写微小裂痕\n- 未来会死的角色：写导致死亡的性格底色\n\n## 关键事件时间线\n| 章节 | 事件 | 涉及角色 | 对番外的约束 |\n|---|---|---|---|\n（从章节摘要中提取关键事件）\n\n## 伏笔状态\n| Hook ID | 类型 | 状态 | 内容 | 预期回收 |\n|---|---|---|---|---|\n\n## 资源账本快照\n（当前资源状态）\n\n---\nmeta:\n  parentBookId: \"{parentBookId}\"\n  parentTitle: \"{正传书名}\"\n  generatedAt: \"{ISO timestamp}\"\n\n要求：\n1. 世界规则完整复制，不压缩——准确性优先\n2. 正典约束必须穷尽，遗漏会导致番外与正传矛盾\n3. 角色快照必须包含信息边界（已知/未知），防止番外中角色引用不该知道的信息";
    let user = format!(
        "正传书名：{title}\n正传ID：{parent_book_id}\n\n## 正传世界设定\n{story_bible}\n\n## 正传当前状态卡\n{current_state}\n\n## 正传资源账本\n{ledger}\n\n## 正传伏笔池\n{hooks}\n\n## 正传章节摘要\n{summaries}\n\n## 正传支线进度\n{subplots}\n\n## 正传情感弧线\n{emotions}\n\n## 正传角色矩阵\n{matrix}",
        title = parent_book.title,
    );
    let response = router
        .chat(
            "parent-canon",
            vec![
                LLMMessage { role: LLMRole::System, content: system.to_string(), tool_calls: None, tool_call_id: None },
                LLMMessage { role: LLMRole::User, content: user, tool_calls: None, tool_call_id: None },
            ],
            0.3,
            None,
        )
        .await?;

    // 确定性 meta 块（LLM 会幻觉时间戳）。
    let meta_block = [
        "",
        "---",
        "meta:",
        &format!("  parentBookId: \"{parent_book_id}\""),
        &format!("  parentTitle: \"{}\"", parent_book.title),
        &format!("  generatedAt: \"{}\"", utc_now_iso()),
    ]
    .join("\n");
    let canon = format!("{content}{meta_block}", content = response.content);

    tokio::fs::write(story_dir.join("parent_canon.md"), &canon)
        .await
        .map_err(|e| e.to_string())?;

    // 父书章节样本 ≥500 字 → 顺带风格向导（吞错）。
    let sample = read_parent_chapter_sample(&parent_dir.join("chapters")).await;
    if sample.encode_utf16().count() >= 500 {
        let _ = generate_style_guide(state, router, builtin_genres_dir, target_book_id, &sample, Some(&parent_book.title)).await;
    }
    Ok(canon)
}

pub async fn import_canon_endpoint(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let parsed: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
    let Some(from_book_id) = parsed.get("fromBookId").and_then(Value::as_str) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "fromBookId is required" })));
    };
    runtime
        .hub
        .broadcast("import:start", &json!({ "bookId": book_id, "type": "canon" }));
    match import_canon(
        &runtime.state,
        &*runtime.effective_router().await,
        &runtime.builtin_genres_dir,
        &book_id,
        from_book_id,
    )
    .await
    {
        Ok(_) => {
            runtime
                .hub
                .broadcast("import:complete", &json!({ "bookId": book_id, "type": "canon" }));
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Err(error) => {
            runtime
                .hub
                .broadcast("import:error", &json!({ "bookId": book_id, "error": error }));
            flat_internal(error)
        }
    }
}

// ── GET /api/v1/books/:id/fanfic ─────────────────────────────────

pub async fn fanfic_show(
    State(runtime): State<BooksRuntime>,
    Path(book_id): Path<String>,
) -> impl IntoResponse {
    let canon_path = runtime.state.book_dir(&book_id).join("story").join("fanfic_canon.md");
    let content = tokio::fs::read_to_string(&canon_path).await.ok();
    (
        StatusCode::OK,
        Json(json!({ "bookId": book_id, "content": content })),
    )
}
