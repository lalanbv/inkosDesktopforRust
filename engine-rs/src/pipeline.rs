//! pipeline 域（编排层）。
//!
//! 自下而上移植自 `packages/core/src/pipeline`：当前含
//! [`persisted_governed_plan`]（治理计划 plan.md 存取 + legacy intent 回退）；
//! runner 编排本体（writeNextChapter 全链路）待 agents 域齐备后迁移。

pub mod chapter_persistence;
pub mod chapter_review_cycle;
pub mod chapter_state_recovery;
pub mod chapter_truth_validation;
pub mod persisted_governed_plan;
