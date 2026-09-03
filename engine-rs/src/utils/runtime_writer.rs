//! 治理运行时工件落盘（context.json / rule-stack.yaml / trace.json）。
//!
//! 移植自 `packages/core/src/utils/runtime-writer.ts`（41 行）。
//! 三件套写入 `story/runtime/chapter-NNNN.*`，是 compose → write 阶段间的
//! 权威交接工件。
//!
//! ## 已知分歧（非 golden 面）
//! TS 用 js-yaml `dump(lineWidth: 120)` 序列化 rule-stack；Rust 用 serde_yaml_ng
//! `to_string`。两者语义等价（字段/值逐一对应），行折叠与引号风格不同——
//! 该文件面向人读与下游 YAML 解析，不参与字节级 golden 差分。

use std::path::{Path, PathBuf};

use crate::models::input_governance::{ChapterTrace, ContextPackage, RuleStack};

/// 落盘结果。对齐 TS `RuntimeArtifactWriteResult`。
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeArtifactWriteResult {
    pub context_path: PathBuf,
    pub rule_stack_path: PathBuf,
    pub trace_path: PathBuf,
}

/// 写入治理工件三件套（目录不存在则递归创建）。
pub async fn write_governed_runtime_artifacts(
    runtime_dir: &Path,
    chapter_number: u32,
    context_package: &ContextPackage,
    rule_stack: &RuleStack,
    trace: &ChapterTrace,
) -> std::io::Result<RuntimeArtifactWriteResult> {
    tokio::fs::create_dir_all(runtime_dir).await?;

    let chapter_slug = format!("chapter-{:04}", chapter_number);
    let context_path = runtime_dir.join(format!("{chapter_slug}.context.json"));
    let rule_stack_path = runtime_dir.join(format!("{chapter_slug}.rule-stack.yaml"));
    let trace_path = runtime_dir.join(format!("{chapter_slug}.trace.json"));

    let context_json = serde_json::to_string_pretty(context_package)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let rule_stack_yaml = serde_yaml_ng::to_string(rule_stack)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let trace_json = serde_json::to_string_pretty(trace)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    let (a, b, c) = tokio::join!(
        tokio::fs::write(&context_path, context_json),
        tokio::fs::write(&rule_stack_path, rule_stack_yaml),
        tokio::fs::write(&trace_path, trace_json),
    );
    a?;
    b?;
    c?;

    Ok(RuntimeArtifactWriteResult {
        context_path,
        rule_stack_path,
        trace_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::input_governance::{
        ContextSource, RuleStackSections, TraceContextTiers, TraceTokenBudget,
    };

    #[tokio::test]
    async fn writes_three_artifacts_with_padded_names() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("runtime");
        let package = ContextPackage {
            chapter: 12,
            selected_context: vec![ContextSource {
                source: "runtime/chapter_memo".into(),
                reason: "memo".into(),
                excerpt: Some("goal=x".into()),
            }],
        };
        let rule_stack = RuleStack {
            layers: vec![],
            sections: RuleStackSections::default(),
            override_edges: vec![],
            active_overrides: vec![],
        };
        let trace = ChapterTrace {
            chapter: 12,
            planner_inputs: vec![],
            composer_inputs: vec![],
            selected_sources: vec!["runtime/chapter_memo".into()],
            prompt_packs: vec![],
            context_tiers: TraceContextTiers::default(),
            token_budget: TraceTokenBudget::default(),
            compression: None,
            notes: vec![],
        };

        let result = write_governed_runtime_artifacts(&runtime, 12, &package, &rule_stack, &trace)
            .await
            .unwrap();
        assert!(result.context_path.ends_with("chapter-0012.context.json"));
        assert!(result.rule_stack_path.ends_with("chapter-0012.rule-stack.yaml"));
        assert!(result.trace_path.ends_with("chapter-0012.trace.json"));

        let context_back: ContextPackage =
            serde_json::from_str(&tokio::fs::read_to_string(&result.context_path).await.unwrap())
                .unwrap();
        assert_eq!(context_back, package);
        let trace_back: ChapterTrace =
            serde_json::from_str(&tokio::fs::read_to_string(&result.trace_path).await.unwrap())
                .unwrap();
        assert_eq!(trace_back.selected_sources, vec!["runtime/chapter_memo"]);
        let yaml_raw = tokio::fs::read_to_string(&result.rule_stack_path).await.unwrap();
        assert!(yaml_raw.contains("layers"));
    }
}
