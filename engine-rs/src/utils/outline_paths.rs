//! Phase 5 (v13) 散文大纲路径解析（outline-paths）。
//!
//! 移植自 `packages/core/src/utils/outline-paths.ts`（340 行）。优先读新布局的
//! prose outline 文件，回退 legacy 路径，保证旧书在过渡期继续可用：
//! - `story/outline/story_frame.md` → 替代 `story_bible.md`
//! - `story/outline/volume_map.md` → 替代 `volume_outline.md`
//! - `story/roles/{主要角色,次要角色}/*.md` → 替代 `character_matrix.md`
//!
//! ## 与 TS 的差异
//! - **读取顺序确定性**：TS `readRoleCards` 用 `Promise.all` 并发 push 到共享数组，
//!   卡片顺序取决于各目录读取完成序（竞态）。Rust 按「主要角色 → 次要角色 →
//!   major → minor」串行序收集——实践上与 TS 完成序一致（首个发起的目录最先完成），
//!   且确定可复现（golden 差分友好）。
//! - `access()` 存在性检查 → `tokio::fs::metadata`。
//! - 字符长度对齐 TS 的 UTF-16 码元计数（`is_current_state_seed_placeholder`
//!   的 600 阈值是行为分支点，须与 JS `String.length` 同源）。

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use tokio::fs;

use crate::utils::language::utf16_len;

/// 角色层级。对齐 TS `RoleCard.tier: "major" | "minor"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "lowercase")]
pub enum RoleTier {
    Major,
    Minor,
}

/// 角色卡。对齐 TS `RoleCard`。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
pub struct RoleCard {
    pub tier: RoleTier,
    pub name: String,
    pub content: String,
}

/// 读文件，任何失败 → fallback。对齐 TS `readOr`。
async fn read_or(path: &Path, fallback: &str) -> String {
    fs::read_to_string(path).await.unwrap_or_else(|_| fallback.to_string())
}

/// 检测书是否使用 Phase 5 新布局（`story/outline/story_frame.md` 存在）。
/// 新布局下 story_bible.md / book_rules.md 是兼容 shim；旧布局则它们是真源。
/// 对齐 TS `isNewLayoutBook`。
pub async fn is_new_layout_book(book_dir: &Path) -> bool {
    fs::metadata(book_dir.join("story").join("outline").join("story_frame.md"))
        .await
        .is_ok()
}

/// 书的架构师地基是否已完整落盘。对齐 TS `isBookFoundationComplete`。
///
/// 「完整」镜像架构师必须产出的五节（story_frame / volume_map / book_rules /
/// pending_hooks / roles）；缺任一即未就绪。roles 检查两个真实读取源任一：
/// `roles/<tier>/` 下的角色卡，或 legacy `character_matrix.md`。
pub async fn is_book_foundation_complete(book_dir: &Path) -> bool {
    let required = [
        book_dir.join("book.json"),
        book_dir.join("story").join("outline").join("story_frame.md"),
        book_dir.join("story").join("outline").join("volume_map.md"),
        book_dir.join("story").join("book_rules.md"),
        book_dir.join("story").join("pending_hooks.md"),
    ];
    for path in &required {
        if fs::metadata(path).await.is_err() {
            return false;
        }
    }
    for tier in ["主要角色", "major", "次要角色", "minor"] {
        let dir = book_dir.join("story").join("roles").join(tier);
        let Ok(mut entries) = fs::read_dir(&dir).await else {
            continue; // 试下一个 locale/tier 目录
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_name().to_string_lossy().ends_with(".md") {
                return true;
            }
        }
    }
    if let Ok(matrix) = fs::read_to_string(book_dir.join("story").join("character_matrix.md")).await {
        if has_legacy_character_matrix_roles(&matrix) {
            return true;
        }
    }
    false
}

/// legacy character_matrix.md 是否承载真实角色（而非 shim 指针/空表）。
/// 逐字移植 TS `hasLegacyCharacterMatrixRoles`。
fn has_legacy_character_matrix_roles(content: &str) -> bool {
    static NONE_LINE: OnceLock<Regex> = OnceLock::new();
    static HEADING: OnceLock<Regex> = OnceLock::new();
    static EXCLUDED_TITLE: OnceLock<Regex> = OnceLock::new();
    // (?i) 前缀：TS /i 标志（none/NONE 等大小写不敏感；中文不受影响）。
    let none_line = NONE_LINE.get_or_init(|| Regex::new(r"(?i)^\(?(none|无|暂无)\)?$").unwrap());
    let heading = HEADING.get_or_init(|| Regex::new(r"^#{2,}\s+(.+)$").unwrap());
    let excluded = EXCLUDED_TITLE.get_or_init(|| {
        Regex::new(r"(?i)^(主要角色|次要角色|major roles?|minor roles?|characters?|角色矩阵)$")
            .unwrap()
    });

    // split(/\r?\n/) → trim → 去空行 → 三类过滤行。
    content
        .split("\r\n")
        .flat_map(|s| s.split('\n'))
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.contains("兼容提示"))
        .filter(|line| !line.contains("story/roles/"))
        .filter(|line| !none_line.is_match(line))
        .any(|line| {
            let Some(m) = heading.captures(line) else {
                return false;
            };
            let title = m.get(1).expect("组 1 必在").as_str().trim();
            let title = title.replace(['*', '`', '#'], "");
            !excluded.is_match(&title)
        })
}

/// 读 story_frame.md，回退 legacy story_bible.md。对齐 TS `readStoryFrame`。
pub async fn read_story_frame(book_dir: &Path, fallback_placeholder: &str) -> String {
    let new_path = book_dir.join("story").join("outline").join("story_frame.md");
    let legacy_path = book_dir.join("story").join("story_bible.md");

    let new_content = read_or(&new_path, "").await;
    if !new_content.trim().is_empty() {
        return new_content;
    }
    read_or(&legacy_path, fallback_placeholder).await
}

/// 读 volume_map.md，回退 legacy volume_outline.md。对齐 TS `readVolumeMap`。
pub async fn read_volume_map(book_dir: &Path, fallback_placeholder: &str) -> String {
    let new_path = book_dir.join("story").join("outline").join("volume_map.md");
    let legacy_path = book_dir.join("story").join("volume_outline.md");

    let new_content = read_or(&new_path, "").await;
    if !new_content.trim().is_empty() {
        return new_content;
    }
    read_or(&legacy_path, fallback_placeholder).await
}

/// 读节奏原则文件（中文或英文变体）。对齐 TS `readRhythmPrinciples`。
pub async fn read_rhythm_principles(book_dir: &Path) -> String {
    let zh_path = book_dir.join("story").join("outline").join("节奏原则.md");
    let en_path = book_dir.join("story").join("outline").join("rhythm_principles.md");

    let zh = read_or(&zh_path, "").await;
    if !zh.trim().is_empty() {
        return zh;
    }
    read_or(&en_path, "").await
}

/// 读 roles/ 目录（中英四个 tier 目录）。无角色时返回空 Vec
/// （如仍用 character_matrix.md 的旧书）。对齐 TS `readRoleCards`。
pub async fn read_role_cards(book_dir: &Path) -> Vec<RoleCard> {
    let roles_root = book_dir.join("story").join("roles");
    // TS 用 Promise.all 并发；Rust 串行序收集（确定性，见模块注释）。
    let dirs: [(&str, RoleTier); 4] = [
        ("主要角色", RoleTier::Major),
        ("次要角色", RoleTier::Minor),
        ("major", RoleTier::Major),
        ("minor", RoleTier::Minor),
    ];
    let mut cards = Vec::new();
    for (dir, tier) in dirs {
        collect_role_dir(&roles_root.join(dir), tier, &mut cards).await;
    }
    cards
}

/// 收集单个 tier 目录下的 .md 角色卡（目录不存在 → no-op；空内容文件跳过）。
async fn collect_role_dir(dir: &Path, tier: RoleTier, out: &mut Vec<RoleCard>) {
    let Ok(mut entries) = fs::read_dir(dir).await else {
        return;
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".md") {
            files.push(dir.join(name.as_ref()));
        }
    }
    for path in files {
        let Ok(content) = fs::read_to_string(&path).await else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }
        let name = path
            .file_name()
            .expect("来自 read_dir 条目")
            .to_string_lossy();
        // TS entry.replace(/\.md$/, "")：仅去尾部 .md（strip_suffix 等价）。
        let name = name.strip_suffix(".md").unwrap_or(&name).to_string();
        out.push(RoleCard {
            tier,
            name,
            content,
        });
    }
}

/// 渲染角色卡为兼容 character_matrix.md 散文消费方的格式。
/// 无角色卡时回退 legacy character_matrix.md 或 placeholder。
/// 对齐 TS `readCharacterContext`。
pub async fn read_character_context(book_dir: &Path, fallback_placeholder: &str) -> String {
    let cards = read_role_cards(book_dir).await;
    if !cards.is_empty() {
        let mut major: Vec<&RoleCard> = Vec::new();
        let mut minor: Vec<&RoleCard> = Vec::new();
        for card in &cards {
            match card.tier {
                RoleTier::Major => major.push(card),
                RoleTier::Minor => minor.push(card),
            }
        }

        let render = |tier_cards: &[&RoleCard], heading: &str| -> String {
            if tier_cards.is_empty() {
                return String::new();
            }
            let sections: Vec<String> = tier_cards
                .iter()
                .map(|card| format!("### {}\n\n{}", card.name, card.content.trim()))
                .collect();
            format!("## {heading}\n\n{}", sections.join("\n\n"))
        };

        let blocks = [
            render(&major, "主要角色 / Major characters"),
            render(&minor, "次要角色 / Minor characters"),
        ];
        let non_empty: Vec<&String> = blocks.iter().filter(|b| !b.is_empty()).collect();
        return non_empty
            .iter()
            .map(|b| b.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
    }

    // 回退：legacy character_matrix.md（其本身可能是 shim 指针）。
    read_or(
        &book_dir.join("story").join("character_matrix.md"),
        fallback_placeholder,
    )
    .await
}

// ---------------------------------------------------------------------------
// Phase 5 consolidation：current_state.md 初始回退。
//
// 架构师整合（7→5 节）后，current_state.md 在建书时只写入极小的占位符。
// 真实内容要等整合器从第 1 章起追加。依赖架构师初始状态的读取方
// （writer phase-1 创作提示、continuity、chapter-analyzer、reviser、composer）
// 在盘上只有种子占位时应代入派生的初始状态块——否则提示词中的
// 「## 当前状态卡」会退化成关于运行时追加行为的元说明。
// ---------------------------------------------------------------------------

/// 架构师 writeFoundationFiles 播种 current_state.md 时发出的标记子串。
/// 读取方凭它判断「还没有真实内容」。
const CURRENT_STATE_SEED_MARKERS: [&str; 2] = ["建书时占位", "Seeded at book creation"];

/// current_state.md 是否仍为种子占位。对齐 TS `isCurrentStateSeedPlaceholder`：
/// 空 → true；trim 后 UTF-16 长度 > 600 → false；否则含任一 seed 标记 → true。
pub fn is_current_state_seed_placeholder(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return true;
    }
    // 阈值与 JS String.length（UTF-16 码元）同源。
    if utf16_len(trimmed) > 600 {
        return false;
    }
    CURRENT_STATE_SEED_MARKERS.iter().any(|marker| trimmed.contains(marker))
}

/// 从角色卡内容提取「当前现状」节。对齐 TS `extractCurrentStateFromRole`：
/// 接受中文（`## 当前现状`）与英文（`## Current_State` / `## Current State`，
/// 大小写不敏感），截到下一个 `## ` 标题。
fn extract_current_state_from_role(content: &str) -> Option<String> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    static NEXT_HEADING: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r"(?im)^##\s*(?:当前现状|Current[_\s]?State)[^\n]*$").unwrap()
    });
    let next_heading = NEXT_HEADING.get_or_init(|| Regex::new(r"(?m)^##\s").unwrap());

    let m = pattern.find(content)?;
    let after = &content[m.end()..];
    // 截到下一个 `## ` 标题（同级或更高级）。
    let raw = match next_heading.find(after) {
        Some(next) => &after[..next.start()],
        None => after,
    };
    let text = raw.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// 从 pending_hooks.md 提取 startChapter=0 的种子伏笔行。对齐 TS
/// `extractSeedHooksFromPendingHooks`（markdown 表格行解析）。
fn extract_seed_hooks_from_pending_hooks(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    static SEPARATOR_ROW: OnceLock<Regex> = OnceLock::new();
    let separator_row = SEPARATOR_ROW.get_or_init(|| Regex::new(r"^\|\s*-").unwrap());

    let mut seed_rows = Vec::new();
    for line in raw.split('\n') {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        // 跳过分隔行（| --- | --- |）。
        if separator_row.is_match(line) {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        // TS split("|").slice(1, -1)：去首尾空段。
        if parts.len() < 3 {
            continue; // cells.length < 2
        }
        let cells: Vec<&str> = parts[1..parts.len() - 1].iter().map(|c| c.trim()).collect();
        if cells.len() < 2 {
            continue;
        }
        let hook_id = cells[0];
        if hook_id.to_lowercase() == "hook_id" || hook_id == "hookId" {
            continue;
        }
        // TS Number.parseInt：宽松前缀解析（"0" / "0.5" / " 0" → 0；"abc" → NaN）。
        if parse_int_prefix(cells[1]) != Some(0) {
            continue;
        }
        // cells[2] 类型；末 cell 备注。
        let notes = cells[cells.len() - 1];
        let mut summary_parts: Vec<&str> = Vec::with_capacity(3);
        if !hook_id.is_empty() {
            summary_parts.push(hook_id);
        }
        if let Some(kind) = cells.get(2).filter(|s| !s.is_empty()) {
            summary_parts.push(kind);
        }
        if !notes.is_empty() {
            summary_parts.push(notes);
        }
        let summary = summary_parts.join(" · ");
        if !summary.is_empty() {
            seed_rows.push(summary);
        }
    }
    seed_rows
}

/// JS `Number.parseInt(s, 10)` 对应物：跳过前导空白，可选符号 + 连续数字前缀。
/// 无数字前缀（NaN）或溢出 i64 → None。
fn parse_int_prefix(s: &str) -> Option<i64> {
    static PREFIX: OnceLock<Regex> = OnceLock::new();
    let prefix = PREFIX.get_or_init(|| Regex::new(r"^([+-]?[0-9]+)").unwrap());
    let m = prefix.find(s.trim_start())?;
    m.as_str().parse::<i64>().ok()
}

/// 读 current_state.md；当文件仍是种子占位（第 0 章，整合器未追加任何内容）时，
/// 从 roles/*.当前现状 + pending_hooks startChapter=0 行派生初始状态块。
/// 对齐 TS `readCurrentStateWithFallback`。
pub async fn read_current_state_with_fallback(book_dir: &Path, fallback_placeholder: &str) -> String {
    let story_dir = book_dir.join("story");
    let current_state_path = story_dir.join("current_state.md");
    let raw = read_or(&current_state_path, "").await;

    if !is_current_state_seed_placeholder(&raw) {
        return raw;
    }

    let pending_hooks_path = story_dir.join("pending_hooks.md");
    let (cards, pending_hooks) = tokio::join!(
        read_role_cards(book_dir),
        read_or(&pending_hooks_path, ""),
    );

    let role_lines: Vec<String> = cards
        .iter()
        .filter_map(|card| {
            extract_current_state_from_role(&card.content).map(|state| {
                let tier_label = if card.tier == RoleTier::Major {
                    "主要"
                } else {
                    "次要"
                };
                // TS state.replace(/\s+/g, " ")：折叠连续空白为单空格。
                let collapsed = state.split_whitespace().collect::<Vec<_>>().join(" ");
                format!("- {}（{tier_label}）：{collapsed}", card.name)
            })
        })
        .collect();

    let hook_lines = extract_seed_hooks_from_pending_hooks(&pending_hooks);

    if role_lines.is_empty() && hook_lines.is_empty() {
        return if !raw.trim().is_empty() {
            raw
        } else {
            fallback_placeholder.to_string()
        };
    }

    let mut parts: Vec<String> = vec!["# 初始状态（第 0 章，由 roles + 种子伏笔派生）".to_string()];
    if !role_lines.is_empty() {
        parts.push("\n## 角色初始位置 / 处境".to_string());
        parts.extend(role_lines);
    }
    if !hook_lines.is_empty() {
        parts.push("\n## 种子伏笔（startChapter = 0）".to_string());
        parts.extend(hook_lines.into_iter().map(|line| format!("- {line}")));
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建 book fixture：story/ 目录 + 指定文件。
    async fn book_fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("临时目录");
        let book = dir.path().join("book");
        for (rel, content) in files {
            let path = book.join(rel);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.expect("建父目录");
            }
            tokio::fs::write(&path, content).await.expect("写文件");
        }
        (dir, book)
    }

    // --- is_new_layout_book / is_book_foundation_complete ---

    #[tokio::test]
    async fn is_new_layout_book_detects_story_frame() {
        let (_tmp, book) = book_fixture(&[("story/outline/story_frame.md", "# 框架")]).await;
        assert!(is_new_layout_book(&book).await);

        let (_tmp, book) = book_fixture(&[("story/story_bible.md", "# 旧书")]).await;
        assert!(!is_new_layout_book(&book).await);
    }

    #[tokio::test]
    async fn foundation_complete_requires_all_five_sections() {
        let (_tmp, book) = book_fixture(&[
            ("book.json", "{}"),
            ("story/outline/story_frame.md", "a"),
            ("story/outline/volume_map.md", "b"),
            ("story/book_rules.md", "c"),
            ("story/pending_hooks.md", "d"),
        ])
        .await;
        // 五节齐但无 roles → 不完整。
        assert!(!is_book_foundation_complete(&book).await);

        // 补 roles 目录 → 完整。
        let (_tmp2, book2) = book_fixture(&[
            ("book.json", "{}"),
            ("story/outline/story_frame.md", "a"),
            ("story/outline/volume_map.md", "b"),
            ("story/book_rules.md", "c"),
            ("story/pending_hooks.md", "d"),
            ("story/roles/主要角色/林动.md", "## 档案\n冷静"),
        ])
        .await;
        assert!(is_book_foundation_complete(&book2).await);

        // 英文 tier 目录同样有效。
        let (_tmp3, book3) = book_fixture(&[
            ("book.json", "{}"),
            ("story/outline/story_frame.md", "a"),
            ("story/outline/volume_map.md", "b"),
            ("story/book_rules.md", "c"),
            ("story/pending_hooks.md", "d"),
            ("story/roles/minor/.keep", ""),
            ("story/roles/major/Bob.md", "## 档案\n冷静"),
        ])
        .await;
        assert!(is_book_foundation_complete(&book3).await);

        // 缺 book.json → 不完整。
        let (_tmp4, book4) = book_fixture(&[
            ("story/outline/story_frame.md", "a"),
            ("story/outline/volume_map.md", "b"),
            ("story/book_rules.md", "c"),
            ("story/pending_hooks.md", "d"),
            ("story/roles/major/Bob.md", "x"),
        ])
        .await;
        assert!(!is_book_foundation_complete(&book4).await);
    }

    #[tokio::test]
    async fn foundation_complete_accepts_legacy_matrix_roles() {
        // 无 roles 目录，但 character_matrix.md 载有真实角色标题。
        let (_tmp, book) = book_fixture(&[
            ("book.json", "{}"),
            ("story/outline/story_frame.md", "a"),
            ("story/outline/volume_map.md", "b"),
            ("story/book_rules.md", "c"),
            ("story/pending_hooks.md", "d"),
            ("story/character_matrix.md", "## 主要角色\n\n### 林动\n\n冷静果决\n"),
        ])
        .await;
        assert!(is_book_foundation_complete(&book).await);
    }

    #[test]
    fn legacy_matrix_roles_filters_shim_noise() {
        // shim 指针 / 兼容提示 / 空占位 / 分组标题 → 不算角色。
        assert!(!has_legacy_character_matrix_roles(
            "# 角色矩阵（兼容提示）\n本文件仅为 story/roles/ 兼容保留\n(none)\n## 主要角色\nmajor roles?\n"
        ));
        // 真实角色标题（含格式噪声字符）→ 算。
        assert!(has_legacy_character_matrix_roles(
            "## 主要角色\n### **林动**\n冷静\n"
        ));
        // 三级以上标题也算（TS /^#{2,}\s+/）。
        assert!(has_legacy_character_matrix_roles("#### `#苏檀儿#\n聪慧\n"));
        // 角色分组标题以外的英文分组名排除（大小写不敏感）。
        assert!(!has_legacy_character_matrix_roles("## Characters\n仅分组"));
        assert!(has_legacy_character_matrix_roles("## 林动\n主角"));
    }

    // --- read_story_frame / read_volume_map / read_rhythm_principles ---

    #[tokio::test]
    async fn read_story_frame_prefers_new_layout() {
        let (_tmp, book) = book_fixture(&[
            ("story/outline/story_frame.md", "新框架"),
            ("story/story_bible.md", "旧圣经"),
        ])
        .await;
        assert_eq!(read_story_frame(&book, "P").await, "新框架");

        // 新文件空白 → 回退 legacy。
        let (_tmp, book) = book_fixture(&[
            ("story/outline/story_frame.md", "  \n"),
            ("story/story_bible.md", "旧圣经"),
        ])
        .await;
        assert_eq!(read_story_frame(&book, "P").await, "旧圣经");

        // 全缺失 → placeholder。
        let (_tmp, book) = book_fixture(&[]).await;
        assert_eq!(read_story_frame(&book, "P").await, "P");
    }

    #[tokio::test]
    async fn read_volume_map_prefers_new_layout() {
        let (_tmp, book) = book_fixture(&[
            ("story/outline/volume_map.md", "新卷图"),
            ("story/volume_outline.md", "旧卷纲"),
        ])
        .await;
        assert_eq!(read_volume_map(&book, "P").await, "新卷图");

        let (_tmp, book) = book_fixture(&[("story/volume_outline.md", "旧卷纲")]).await;
        assert_eq!(read_volume_map(&book, "P").await, "旧卷纲");

        let (_tmp, book) = book_fixture(&[]).await;
        assert_eq!(read_volume_map(&book, "P").await, "P");
    }

    #[tokio::test]
    async fn read_rhythm_principles_zh_then_en() {
        let (_tmp, book) = book_fixture(&[
            ("story/outline/节奏原则.md", "中文节奏"),
            ("story/outline/rhythm_principles.md", "en rhythm"),
        ])
        .await;
        assert_eq!(read_rhythm_principles(&book).await, "中文节奏");

        let (_tmp, book) =
            book_fixture(&[("story/outline/rhythm_principles.md", "en rhythm")]).await;
        assert_eq!(read_rhythm_principles(&book).await, "en rhythm");

        let (_tmp, book) = book_fixture(&[]).await;
        assert_eq!(read_rhythm_principles(&book).await, "");
    }

    // --- read_role_cards / read_character_context ---

    #[tokio::test]
    async fn read_role_cards_collects_all_four_dirs() {
        let (_tmp, book) = book_fixture(&[
            ("story/roles/主要角色/林动.md", "冷静"),
            ("story/roles/主要角色/应欢欢.md", "古灵精怪"),
            ("story/roles/次要角色/苏檀儿.md", "聪慧"),
            ("story/roles/major/Bob.md", "calm"),
            ("story/roles/minor/Alice.md", "smart"),
            ("story/roles/主要角色/空.md", "   "),
            ("story/roles/主要角色/readme.txt", "非 md"),
        ])
        .await;

        let cards = read_role_cards(&book).await;
        let mut names: Vec<(&str, RoleTier)> = cards
            .iter()
            .map(|c| (c.name.as_str(), c.tier))
            .collect();
        // readdir 返回 OS 目录序（与 TS fs.readdir 一致，无排序保证）——按名排序后断言。
        names.sort_by_key(|(name, _)| (*name).to_string());
        assert_eq!(
            names,
            vec![
                ("Alice", RoleTier::Minor),
                ("Bob", RoleTier::Major),
                ("应欢欢", RoleTier::Major),
                ("林动", RoleTier::Major),
                ("苏檀儿", RoleTier::Minor),
            ]
        );
        let lindong = cards.iter().find(|c| c.name == "林动").expect("林动卡");
        assert_eq!(lindong.content, "冷静");
    }

    #[tokio::test]
    async fn read_character_context_renders_groups() {
        let (_tmp, book) = book_fixture(&[
            ("story/roles/主要角色/林动.md", " 冷静\n果决 "),
            ("story/roles/次要角色/苏檀儿.md", "聪慧"),
        ])
        .await;

        let out = read_character_context(&book, "P").await;
        assert_eq!(
            out,
            "## 主要角色 / Major characters\n\n### 林动\n\n冷静\n果决\n\n## 次要角色 / Minor characters\n\n### 苏檀儿\n\n聪慧"
        );
    }

    #[tokio::test]
    async fn read_character_context_single_group_has_no_separator() {
        let (_tmp, book) =
            book_fixture(&[("story/roles/major/Bob.md", "calm")]).await;
        let out = read_character_context(&book, "P").await;
        assert_eq!(out, "## 主要角色 / Major characters\n\n### Bob\n\ncalm");
    }

    #[tokio::test]
    async fn read_character_context_falls_back_to_legacy_matrix() {
        let (_tmp, book) =
            book_fixture(&[("story/character_matrix.md", "## 主要角色\n### 林动\n冷静")]).await;
        assert_eq!(
            read_character_context(&book, "P").await,
            "## 主要角色\n### 林动\n冷静"
        );

        let (_tmp, book) = book_fixture(&[]).await;
        assert_eq!(read_character_context(&book, "P").await, "P");
    }

    // --- is_current_state_seed_placeholder ---

    #[test]
    fn seed_placeholder_detection() {
        assert!(is_current_state_seed_placeholder(""));
        assert!(is_current_state_seed_placeholder("   \n "));
        assert!(is_current_state_seed_placeholder("# 当前状态\n建书时占位，待整合器追加"));
        assert!(is_current_state_seed_placeholder(
            "# Current State\nSeeded at book creation"
        ));
        // 长文件（>600 UTF-16 码元）即使含标记也不是占位。
        let long = "x".repeat(601);
        assert!(!is_current_state_seed_placeholder(&long));
        // 短而无标记 → 非占位（真实内容）。
        assert!(!is_current_state_seed_placeholder("林动已经突破至凝魂境"));
    }

    #[test]
    fn seed_placeholder_uses_utf16_threshold() {
        // 前缀 "# 状态\n建书时占位\n" = 1+1+2+1+5+1 = 11 码元。
        // 589 个「稳」→ 总 600（不超阈值，含标记 → 占位）。
        let zh = format!("# 状态\n建书时占位\n{}", "稳".repeat(589));
        assert!(is_current_state_seed_placeholder(&zh));
        // 590 → 总 601（超阈值 → 非占位）。
        let zh_over = format!("# 状态\n建书时占位\n{}", "稳".repeat(590));
        assert!(!is_current_state_seed_placeholder(&zh_over));
    }

    // --- extract_current_state_from_role（经 read_current_state_with_fallback 触达）---

    #[tokio::test]
    async fn current_state_fallback_passes_through_real_content() {
        let (_tmp, book) =
            book_fixture(&[("story/current_state.md", "# 当前状态\n林动：凝魂境三层")]).await;
        assert_eq!(
            read_current_state_with_fallback(&book, "P").await,
            "# 当前状态\n林动：凝魂境三层"
        );
    }

    #[tokio::test]
    async fn current_state_fallback_derives_from_roles_and_seed_hooks() {
        let hooks_table = "| hook_id | 起始章 | 类型 | 描述 | 目标章 | 备注 |\n\
             | --- | --- | --- | --- | --- | --- |\n\
             | H1 | 0 | 世界 | 神秘玉佩 | 12 | 开局悬念 |\n\
             | H2 | 0 | 情感 | 师徒羁绊 | 30 |  |\n\
             | H3 | 5 | 力量 | 剑意觉醒 | 20 | 非种子 |\n\
             | H4 | 1 | 无效 | 无起始 | 9 | 非零起始 |\n\
             | hook_id | 0 | 表头 | 行 | 1 | 忽略 |\n";
        let (_tmp, book) = book_fixture(&[
            ("story/current_state.md", "# 当前状态\n建书时占位"),
            (
                "story/roles/主要角色/林动.md",
                "## 档案\n冷静\n\n## 当前现状\n在青云宗\n修炼\n\n## 其他\n略",
            ),
            ("story/roles/次要角色/苏檀儿.md", "## Current State\nAt home"),
            ("story/pending_hooks.md", hooks_table),
        ])
        .await;

        let out = read_current_state_with_fallback(&book, "P").await;
        let expected = "# 初始状态（第 0 章，由 roles + 种子伏笔派生）\n\
            \n## 角色初始位置 / 处境\n\
            - 林动（主要）：在青云宗 修炼\n\
            - 苏檀儿（次要）：At home\n\
            \n## 种子伏笔（startChapter = 0）\n\
            - H1 · 世界 · 开局悬念\n\
            - H2 · 情感";
        assert_eq!(out, expected);
    }

    #[tokio::test]
    async fn current_state_fallback_seed_placeholder_without_derivations() {
        // 种子占位 + 无 roles / hooks → 返回原 raw（trim 后非空）。
        let (_tmp, book) =
            book_fixture(&[("story/current_state.md", "# 当前状态\n建书时占位")]).await;
        assert_eq!(
            read_current_state_with_fallback(&book, "P").await,
            "# 当前状态\n建书时占位"
        );

        // 空文件 + 无派生源 → placeholder。
        let (_tmp, book) = book_fixture(&[]).await;
        assert_eq!(read_current_state_with_fallback(&book, "P").await, "P");
    }

    // --- parse_int_prefix（parseInt parity）---

    #[test]
    fn parse_int_prefix_matches_js_parseint() {
        assert_eq!(parse_int_prefix("0"), Some(0));
        assert_eq!(parse_int_prefix(" 0 "), Some(0)); // 前导空白跳过
        assert_eq!(parse_int_prefix("0.5"), Some(0)); // 前缀 0（JS parseInt("0.5")=0）
        assert_eq!(parse_int_prefix("5"), Some(5));
        assert_eq!(parse_int_prefix("-1"), Some(-1));
        assert_eq!(parse_int_prefix("+3"), Some(3));
        assert_eq!(parse_int_prefix("abc"), None); // NaN
        assert_eq!(parse_int_prefix(""), None);
        assert_eq!(parse_int_prefix("--1"), None); // 双符号 NaN
    }
}
