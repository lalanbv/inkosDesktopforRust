//! 同人审查维度配置（fanfic-dimensions）。
//!
//! 移植自 `packages/core/src/agents/fanfic-dimensions.ts`（87 行，纯逻辑）。
//! 按同人模式（canon/au/ooc/cp）给出维度 34-37 的激活/严重度/注记，
//! 并停用番外维度 28-31（同人模式下番外检查不适用）。
//! ContinuityAuditor 的 `buildDimensionList` 消费。

use std::collections::BTreeMap;

use serde::Serialize;
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

use super::continuity::AuditSeverity;
use crate::models::book::FanficMode;

/// 同人维度定义。对齐 TS `FANFIC_DIMENSIONS`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanficDimensionDef {
    pub id: u32,
    pub name: &'static str,
    pub base_note: &'static str,
}

/// 同人专用审查维度（34-37）。
pub const FANFIC_DIMENSIONS: [FanficDimensionDef; 4] = [
    FanficDimensionDef {
        id: 34,
        name: "角色还原度",
        base_note: "检查角色的语癖、说话风格、行为模式是否与 fanfic_canon.md 角色档案一致。偏离必须有情境驱动。",
    },
    FanficDimensionDef {
        id: 35,
        name: "世界规则遵守",
        base_note: "检查章节内容是否违反 fanfic_canon.md 中的世界规则（地理、力量体系、阵营关系）。",
    },
    FanficDimensionDef {
        id: 36,
        name: "关系动态",
        base_note: "检查角色之间的关系互动是否合理，是否与 fanfic_canon.md 中标注的关键关系一致或有合理发展。",
    },
    FanficDimensionDef {
        id: 37,
        name: "正典事件一致性",
        base_note: "检查章节是否与 fanfic_canon.md 关键事件时间线矛盾。",
    },
];

/// 同人维度配置。对齐 TS `FanficDimensionConfig`。
/// Map 用 BTreeMap：序列化键序确定（golden 差分友好），语义与 TS Map 等价。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct FanficDimensionConfig {
    pub active_ids: Vec<u32>,
    pub severity_overrides: BTreeMap<u32, AuditSeverity>,
    pub deactivated_ids: Vec<u32>,
    pub notes: BTreeMap<u32, String>,
}

/// 模式 → 维度严重度映射。逐字移植 TS `SEVERITY_MAP`。
fn severity_for(mode: FanficMode, dim: u32) -> AuditSeverity {
    match (mode, dim) {
        (FanficMode::Canon, 34) | (FanficMode::Canon, 35) | (FanficMode::Canon, 37) => {
            AuditSeverity::Critical
        }
        (FanficMode::Au, 34) => AuditSeverity::Critical,
        (FanficMode::Cp, 36) => AuditSeverity::Critical,
        (FanficMode::Canon | FanficMode::Au, 36)
        | (FanficMode::Ooc, 35)
        | (FanficMode::Ooc, 36)
        | (FanficMode::Cp, 34)
        | (FanficMode::Cp, 35) => AuditSeverity::Warning,
        (FanficMode::Au, 35)
        | (FanficMode::Au, 37)
        | (FanficMode::Ooc, 34)
        | (FanficMode::Ooc, 37)
        | (FanficMode::Cp, 37) => AuditSeverity::Info,
        _ => AuditSeverity::Warning, // 不可达（34-37 之外无条目）
    }
}

/// 番外维度（28-31）：同人模式下停用（同作者番外专用）。
const SPINOFF_DIMS: [u32; 4] = [28, 29, 30, 31];

/// 内建 OOC 检查（维度 1）：ooc 模式放宽 / canon 模式收紧。
const OOC_DIM: u32 = 1;

/// 严重度中文注记。逐字移植 TS `severityLabel` 三元链。
fn severity_label(severity: AuditSeverity) -> &'static str {
    match severity {
        AuditSeverity::Critical => "（严格检查）",
        AuditSeverity::Info => "（仅记录，不判定失败）",
        AuditSeverity::Warning => "（警告级别）",
    }
}

/// 取同人维度配置。对齐 TS `getFanficDimensionConfig`。
///
/// `_allowed_deviations` 当前未被 TS 实现使用（签名预留），保留参数以对齐调用面。
pub fn get_fanfic_dimension_config(
    mode: FanficMode,
    _allowed_deviations: &[String],
) -> FanficDimensionConfig {
    let mut severity_overrides: BTreeMap<u32, AuditSeverity> = BTreeMap::new();
    let mut notes: BTreeMap<u32, String> = BTreeMap::new();

    for dim in &FANFIC_DIMENSIONS {
        let severity = severity_for(mode, dim.id);
        severity_overrides.insert(dim.id, severity);
        notes.insert(dim.id, format!("{} {}", dim.base_note, severity_label(severity)));
    }

    // OOC 模式放宽内建 OOC 检查（维度 1 → info）。
    if mode == FanficMode::Ooc {
        severity_overrides.insert(OOC_DIM, AuditSeverity::Info);
        notes.insert(
            OOC_DIM,
            "OOC模式下角色可偏离性格底色，此维度仅记录不判定失败。参照 fanfic_canon.md 角色档案评估偏离程度。".to_string(),
        );
    }

    // 原作向（canon）收紧内建 OOC 检查。
    if mode == FanficMode::Canon {
        notes.insert(
            OOC_DIM,
            "原作向同人：角色必须严格遵守性格底色。参照 fanfic_canon.md 角色档案中的性格底色和行为模式。".to_string(),
        );
    }

    FanficDimensionConfig {
        active_ids: FANFIC_DIMENSIONS.iter().map(|d| d.id).collect(),
        severity_overrides,
        deactivated_ids: SPINOFF_DIMS.to_vec(),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deviations() -> Vec<String> {
        vec!["口头禅".to_string()]
    }

    #[test]
    fn canon_mode_severities() {
        let cfg = get_fanfic_dimension_config(FanficMode::Canon, &deviations());
        assert_eq!(cfg.active_ids, vec![34, 35, 36, 37]);
        assert_eq!(cfg.deactivated_ids, vec![28, 29, 30, 31]);
        assert_eq!(cfg.severity_overrides[&34], AuditSeverity::Critical);
        assert_eq!(cfg.severity_overrides[&35], AuditSeverity::Critical);
        assert_eq!(cfg.severity_overrides[&36], AuditSeverity::Warning);
        assert_eq!(cfg.severity_overrides[&37], AuditSeverity::Critical);
        // canon：维度 1 收紧注记。
        assert!(cfg.notes[&1].contains("原作向同人"));
        assert!(cfg.notes[&1].contains("性格底色和行为模式"));
    }

    #[test]
    fn au_cp_ooc_mode_severities() {
        let au = get_fanfic_dimension_config(FanficMode::Au, &[]);
        assert_eq!(au.severity_overrides[&34], AuditSeverity::Critical);
        assert_eq!(au.severity_overrides[&35], AuditSeverity::Info);
        assert_eq!(au.severity_overrides[&37], AuditSeverity::Info);

        let cp = get_fanfic_dimension_config(FanficMode::Cp, &[]);
        assert_eq!(cp.severity_overrides[&36], AuditSeverity::Critical);
        assert_eq!(cp.severity_overrides[&34], AuditSeverity::Warning);
        assert_eq!(cp.severity_overrides[&37], AuditSeverity::Info);

        let ooc = get_fanfic_dimension_config(FanficMode::Ooc, &[]);
        assert_eq!(ooc.severity_overrides[&34], AuditSeverity::Info);
        // ooc：维度 1 降为 info + 专用注记。
        assert_eq!(ooc.severity_overrides[&1], AuditSeverity::Info);
        assert!(ooc.notes[&1].contains("仅记录不判定失败"));
    }

    #[test]
    fn notes_carry_base_note_and_label() {
        let cfg = get_fanfic_dimension_config(FanficMode::Canon, &[]);
        let note34 = &cfg.notes[&34];
        assert!(note34.starts_with("检查角色的语癖、说话风格、行为模式"));
        assert!(note34.ends_with("（严格检查）"));
        let note36 = &cfg.notes[&36];
        assert!(note36.ends_with("（警告级别）"));
    }

    #[test]
    fn allowed_deviations_param_is_inert() {
        // 参数未被使用（对齐 TS），两次调用结果一致。
        let a = get_fanfic_dimension_config(FanficMode::Au, &deviations());
        let b = get_fanfic_dimension_config(FanficMode::Au, &[]);
        assert_eq!(a, b);
    }
}
