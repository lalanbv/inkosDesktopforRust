//! 伏笔陈旧/受阻检测（Phase 7）。
//!
//! 移植自 `packages/core/src/utils/hook-stale-detection.ts`（168 行，纯函数）。
//! 依赖已移植的 [`HookRecord`] + [`resolve_half_life_chapters`]。
//!
//! 给定当前 hook 列表 + 当前章节号，返回 hookId → 诊断标志（stale/blocked/missingUpstream/distance/halfLife/blockedDistance）。
//! 纯函数，不持久化——结果仅用于渲染 markdown。

use crate::models::runtime_state::{HookRecord, HookStatus};
use crate::utils::hook_promotion::resolve_half_life_chapters;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct HookDiagnostics {
    pub stale: bool,
    pub blocked: bool,
    pub missing_upstream: Vec<String>,
    pub distance: u32,
    pub half_life: u32,
    /// 被阻塞的章节数（blocked=false 时为 0）。
    pub blocked_distance: u32,
}

fn is_resolved(hook: &HookRecord) -> bool {
    matches!(hook.status, HookStatus::Resolved)
}

/// 计算所有 hook 的诊断标志。
pub fn compute_hook_diagnostics(
    hooks: &[HookRecord],
    current_chapter: u32,
) -> HashMap<String, HookDiagnostics> {
    // byId 索引（HookRecord 强类型，hookId 唯一）
    let by_id: HashMap<&str, &HookRecord> = hooks.iter().map(|h| (h.hook_id.as_str(), h)).collect();
    let mut result: HashMap<String, HookDiagnostics> = HashMap::new();

    for hook in hooks {
        let half_life = resolve_half_life_chapters(hook);
        let planted_chapter = hook.start_chapter;
        let distance = current_chapter.saturating_sub(planted_chapter);

        // stale：超过半衰期且未回收且确已种下（startChapter>0）
        let stale = !is_resolved(hook) && planted_chapter > 0 && distance > half_life;

        // blocked：depends_on 引用的上游未种下或未回收
        let mut missing_upstream: Vec<String> = Vec::new();
        let mut upstream_reference_chapters: Vec<u32> = Vec::new();
        for upstream_id in hook.depends_on.clone().unwrap_or_default() {
            match by_id.get(upstream_id.as_str()) {
                None => {
                    // 上游完全缺失 → 自种下起即受阻
                    missing_upstream.push(upstream_id);
                    upstream_reference_chapters.push(planted_chapter);
                }
                Some(upstream) => {
                    let upstream_resolved = is_resolved(upstream);
                    let upstream_planted = upstream.start_chapter > 0
                        && upstream.start_chapter <= current_chapter;
                    if !upstream_planted || !upstream_resolved {
                        missing_upstream.push(upstream_id);
                        let reference = if upstream_planted { upstream.start_chapter } else { planted_chapter };
                        upstream_reference_chapters.push(reference);
                    }
                }
            }
        }
        let blocked = !missing_upstream.is_empty() && !is_resolved(hook);

        // blockedDistance = 距最早未清上游引用章节的章节数（min 引用 → max 距离）
        let mut blocked_distance = 0u32;
        if blocked && !upstream_reference_chapters.is_empty() {
            let earliest = *upstream_reference_chapters.iter().min().unwrap();
            blocked_distance = current_chapter.saturating_sub(earliest);
        }

        result.insert(hook.hook_id.clone(), HookDiagnostics {
            stale,
            blocked,
            missing_upstream,
            distance,
            half_life,
            blocked_distance,
        });
    }

    result
}

/// 渲染诊断标志为紧凑 marker（附加到表格单元格）。无标志则空串。
pub fn render_hook_diagnostic_marker(
    diagnostics: &HookDiagnostics,
    language: crate::utils::language::WritingLanguage,
) -> String {
    use crate::utils::language::WritingLanguage;
    let mut tokens: Vec<String> = Vec::new();
    if diagnostics.stale {
        tokens.push(match language {
            WritingLanguage::En => format!("stale (d={}/half={})", diagnostics.distance, diagnostics.half_life),
            WritingLanguage::Zh => format!("过期 (距={}/半衰={})", diagnostics.distance, diagnostics.half_life),
        });
    }
    if diagnostics.blocked {
        let missing = diagnostics.missing_upstream.join(", ");
        // blockedDistance 嵌入 marker（reviewer 5/6 章阈值判定读取此 token，格式 load-bearing）
        let distance_token = if diagnostics.blocked_distance > 0 {
            match language {
                WritingLanguage::En => format!(" (blocked {} chapters)", diagnostics.blocked_distance),
                WritingLanguage::Zh => format!(" (已阻 {} 章)", diagnostics.blocked_distance),
            }
        } else {
            String::new()
        };
        tokens.push(match language {
            WritingLanguage::En => format!("blocked on {}{}", missing, distance_token),
            WritingLanguage::Zh => format!("受阻于 {}{}", missing, distance_token),
        });
    }
    tokens.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(id: &str, start: u32, status: HookStatus, depends: Option<Vec<&str>>) -> HookRecord {
        HookRecord {
            kind: None,
            hook_id: id.into(),
            start_chapter: start,
            hook_type: "plot".into(),
            status,
            status_raw: String::new(),
            last_advanced_chapter: 0,
            expected_payoff: String::new(),
            payoff_timing: None,
            notes: String::new(),
            depends_on: depends.map(|d| d.iter().map(|s| s.to_string()).collect()),
            pays_off_in_arc: None,
            core_hook: None,
            half_life_chapters: None,
            advanced_count: None,
            promoted: None,
        }
    }

    #[test]
    fn stale_when_past_halflife() {
        // 默认 timing=None → halfLife=30；planted chapter 1，current 35 → distance 34 > 30 → stale
        let h = hook("h1", 1, HookStatus::Open, None);
        let d = compute_hook_diagnostics(&[h], 35);
        let diag = d.get("h1").unwrap();
        assert!(diag.stale);
        assert_eq!(diag.half_life, 30);
        assert_eq!(diag.distance, 34);
    }

    #[test]
    fn not_stale_when_resolved() {
        let h = hook("h1", 1, HookStatus::Resolved, None);
        let d = compute_hook_diagnostics(&[h], 100);
        assert!(!d.get("h1").unwrap().stale);
    }

    #[test]
    fn seed_not_stale_before_planting() {
        // startChapter 0 → pre-planting seed，不判 stale
        let h = hook("h1", 0, HookStatus::Open, None);
        let d = compute_hook_diagnostics(&[h], 100);
        assert!(!d.get("h1").unwrap().stale);
    }

    #[test]
    fn blocked_when_upstream_unresolved() {
        let upstream = hook("up", 1, HookStatus::Open, None);
        let downstream = hook("down", 2, HookStatus::Open, Some(vec!["up"]));
        let d = compute_hook_diagnostics(&[upstream, downstream], 5);
        let diag = d.get("down").unwrap();
        assert!(diag.blocked);
        assert_eq!(diag.missing_upstream, vec!["up".to_string()]);
        // upstream planted at 1，current 5 → blockedDistance 4
        assert_eq!(diag.blocked_distance, 4);
    }

    #[test]
    fn blocked_distance_uses_own_planting_when_upstream_missing() {
        // 上游完全缺失 → 自种下起受阻
        let h = hook("h1", 3, HookStatus::Open, Some(vec!["ghost"]));
        let d = compute_hook_diagnostics(&[h], 8);
        let diag = d.get("h1").unwrap();
        assert!(diag.blocked);
        assert_eq!(diag.blocked_distance, 5); // 8 - 3
    }

    #[test]
    fn not_blocked_when_upstream_resolved() {
        let upstream = hook("up", 1, HookStatus::Resolved, None);
        let downstream = hook("down", 2, HookStatus::Open, Some(vec!["up"]));
        let d = compute_hook_diagnostics(&[upstream, downstream], 5);
        assert!(!d.get("down").unwrap().blocked);
    }

    #[test]
    fn marker_renders_stale_and_blocked() {
        use crate::utils::language::WritingLanguage;
        let diag = HookDiagnostics {
            stale: true,
            blocked: true,
            missing_upstream: vec!["up".into()],
            distance: 12,
            half_life: 10,
            blocked_distance: 4,
        };
        let zh = render_hook_diagnostic_marker(&diag, WritingLanguage::Zh);
        assert!(zh.contains("过期"));
        assert!(zh.contains("受阻于 up"));
        assert!(zh.contains("已阻 4 章"));

        let en = render_hook_diagnostic_marker(&diag, WritingLanguage::En);
        assert!(en.contains("stale (d=12/half=10)"));
        assert!(en.contains("blocked on up"));
        assert!(en.contains("blocked 4 chapters"));
    }

    #[test]
    fn marker_empty_when_clean() {
        use crate::utils::language::WritingLanguage;
        let diag = HookDiagnostics {
            stale: false,
            blocked: false,
            missing_upstream: vec![],
            distance: 0,
            half_life: 30,
            blocked_distance: 0,
        };
        assert_eq!(render_hook_diagnostic_marker(&diag, WritingLanguage::Zh), "");
    }
}
