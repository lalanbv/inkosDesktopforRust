//! 自动化模式。
//!
//! 移植自 `packages/core/src/interaction/modes.ts`。

use serde::{Deserialize, Serialize};
#[cfg(feature = "export-bindings")]
use ts_rs::TS;

/// 自动化模式。对齐 TS `z.enum(["auto","semi","manual"])`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(TS))]
#[cfg_attr(feature = "export-bindings", ts(export, type = "\"auto\" | \"semi\" | \"manual\""))]
pub enum AutomationMode {
    #[serde(rename = "auto")] Auto,
    #[serde(rename = "semi")] Semi,
    #[serde(rename = "manual")] Manual,
}

impl AutomationMode {
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "semi" => Some(Self::Semi),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// 规范化自动化模式：非法值 → fallback（默认 Semi）。
pub fn normalize_automation_mode(mode: Option<&str>, fallback: AutomationMode) -> AutomationMode {
    mode.and_then(AutomationMode::from_str_lossy).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_valid_and_invalid() {
        assert_eq!(normalize_automation_mode(Some("auto"), AutomationMode::Semi), AutomationMode::Auto);
        assert_eq!(normalize_automation_mode(Some("bogus"), AutomationMode::Manual), AutomationMode::Manual);
        assert_eq!(normalize_automation_mode(None, AutomationMode::Semi), AutomationMode::Semi);
    }
}
