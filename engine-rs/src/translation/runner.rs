//! 翻译执行流（runner.ts）：逐章批翻译 → 术语合并落盘 → 章节评审 → 报告。

use std::collections::HashMap;
use std::path::Path;

use crate::production::{
    commit_production_artifacts, write_production_run_snapshot,
    CreateRunInput, ProductionKind, ProductionRunSnapshot, ProductionRunStatus,
};
use crate::translation::run_store::*;
use crate::translation::types::*;

pub async fn run_translation_project(
    project_root: &Path,
    project_id: &str,
    model: &dyn TranslationModelPort,
    batch_size: Option<usize>,
) -> Result<RunTranslationProjectResult, String> {
    // 143 号：TS 运行快照三点（running → 章级进度 → complete/failed）。
    let run_path = format!("translations/{project_id}/status.json");
    let base_artifacts = vec![
        format!("translations/{project_id}/manifest.json"),
        format!("translations/{project_id}/glossary.json"),
    ];
    write_production_run_snapshot(
        project_root,
        &run_path,
        &ProductionRunSnapshot::create(CreateRunInput {
            kind: ProductionKind::Translation,
            id: project_id.to_string(),
            status: ProductionRunStatus::Running,
            stage: "translate".to_string(),
            artifacts: base_artifacts.clone(),
            observations: Vec::new(),
            model: None,
            skill_ids: None,
            resume_cursor: None,
            error: None,
        }),
    )
    .await
    .map_err(|e| e.to_string())?;

    match run_translation_inner(project_root, project_id, model, batch_size, &run_path, &base_artifacts).await {
        Ok(result) => Ok(result),
        Err(error) => {
            let snapshot = ProductionRunSnapshot::create(CreateRunInput {
                kind: ProductionKind::Translation,
                id: project_id.to_string(),
                status: ProductionRunStatus::Failed,
                stage: "translate".to_string(),
                artifacts: base_artifacts.clone(),
                observations: Vec::new(),
                model: None,
                skill_ids: None,
                resume_cursor: None,
                error: Some(error.to_string()),
            });
            let _ = write_production_run_snapshot(project_root, &run_path, &snapshot).await;
            Err(error)
        }
    }
}

async fn run_translation_inner(
    project_root: &Path,
    project_id: &str,
    model: &dyn TranslationModelPort,
    batch_size: Option<usize>,
    run_path: &str,
    base_artifacts: &[String],
) -> Result<RunTranslationProjectResult, String> {
    let mut manifest = load_translation_manifest(project_root, project_id)
        .await
        .map_err(|e| match e {
            LoadManifestError::NotFound => format!("translation project not found for {project_id}"),
            LoadManifestError::BadPayload(message) => message,
        })?;
    let mut glossary = load_translation_glossary(project_root, project_id).await;
    let mut report_lines: Vec<String> = vec!["# Translation Review".to_string(), String::new()];
    let mut translated_segments = 0usize;
    let mut reviewed_chapters = 0usize;
    let batch_size = batch_size.unwrap_or(8).clamp(1, 32);

    for chapter_info in manifest.chapters.clone() {
        let source = load_translation_chapter(project_root, &chapter_info.source_path).await?;
        let translated = match load_translation_chapter(project_root, &chapter_info.translated_path).await {
            Ok(chapter) => chapter,
            Err(_) => TranslationChapterFile { segments: Vec::new(), ..source.clone() },
        };
        let mut translated_by_index: HashMap<u32, TranslationSegment> = translated
            .segments
            .into_iter()
            .map(|segment| (segment.index, segment))
            .collect();
        // 968 号：译后标题（TS translatedTitle——首轮结果即采用，后续批保留）。
        let mut translated_title = translated.title.clone();
        let pending: Vec<&TranslationSegment> = source
            .segments
            .iter()
            .filter(|segment| {
                translated_by_index
                    .get(&segment.index)
                    .and_then(|s| s.target.as_deref())
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .is_none()
            })
            .collect();

        let mut processed = 0usize;
        for batch in pending.chunks(batch_size) {
            let batch_owned: Vec<TranslationSegment> = batch.iter().map(|segment| (*segment).clone()).collect();
            let result = model
                .translate_segments(TranslateSegmentsInput {
                    source_language: &manifest.source_language,
                    target_language: &manifest.target_language,
                    chapter_title: &source.title,
                    segments: &batch_owned,
                    glossary: &glossary,
                })
                .await?;
            // 968 号：严格校验后的结果段号必在批内且齐全——未知/重复/缺失/空 target
            // 已在模型端口硬失败（TS runner 同语义）。
            for item in result.segments {
                let Some(original) = source.segments.iter().find(|s| s.index == item.index) else {
                    return Err(format!("Translation source segment {} is missing.", item.index));
                };
                translated_by_index.insert(
                    item.index,
                    TranslationSegment {
                        index: original.index,
                        source: original.source.clone(),
                        target: Some(item.target),
                        notes: item.notes,
                    },
                );
                translated_segments += 1;
            }
            if let Some(title) = result.chapter_title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                translated_title = title.to_string();
            }
            if !result.glossary.is_empty() {
                let mut merged = glossary.clone();
                merged.extend(result.glossary.iter().cloned());
                glossary = merge_glossary_terms(&merged);
            }
            let ordered_segments: Vec<TranslationSegment> = source
                .segments
                .iter()
                .map(|segment| translated_by_index.get(&segment.index).cloned().unwrap_or_else(|| segment.clone()))
                .collect();
            // 143 号：章译文 + 术语表同事务单点（TS saveTranslationProgress）。
            // 968 号：标题用译后标题（TS `{...source, title: translatedTitle}`）。
            save_translation_progress(
                project_root,
                project_id,
                &chapter_info.translated_path,
                &TranslationChapterFile { title: translated_title.clone(), segments: ordered_segments, ..source.clone() },
                &glossary,
            )
            .await?;
            processed += batch.len();
            let snapshot = ProductionRunSnapshot::create(CreateRunInput {
                kind: ProductionKind::Translation,
                id: project_id.to_string(),
                status: ProductionRunStatus::Running,
                stage: "translate".to_string(),
                artifacts: {
                    let mut arts = base_artifacts.to_vec();
                    arts.push(chapter_info.translated_path.clone());
                    arts
                },
                observations: Vec::new(),
                model: None,
                skill_ids: None,
                resume_cursor: Some(format!("{}:{}", chapter_info.number, processed)),
                error: None,
            });
            write_production_run_snapshot(project_root, run_path, &snapshot)
                .await
                .map_err(|e| e.to_string())?;
        }

        let completed_chapter =
            load_translation_chapter(project_root, &chapter_info.translated_path).await?;
        let mut status = TranslationChapterStatus::Translated;
        // 967 号：本章审校摘要/观察；968 号：未审校即清空（TS updateChapterProgress
        // 以 undefined 覆写同语义）。
        let mut reviewed: Option<(String, Vec<serde_json::Value>)> = None;
        if completed_chapter
            .segments
            .iter()
            .any(|segment| segment.target.as_deref().map(str::trim).is_some_and(|t| !t.is_empty()))
        {
            // 968 号：审校输入标题=译后标题（TS `chapterTitle: completedChapter.title`）。
            let review = model
                .review_chapter(ReviewChapterInput {
                    source_language: &manifest.source_language,
                    target_language: &manifest.target_language,
                    chapter_title: &completed_chapter.title,
                    segments: &completed_chapter.segments,
                    glossary: &glossary,
                })
                .await?;
            reviewed_chapters += 1;
            status = if review.passed {
                TranslationChapterStatus::Reviewed
            } else {
                TranslationChapterStatus::Translated
            };
            report_lines.push(format!("## {}", completed_chapter.title));
            report_lines.push(String::new());
            report_lines.push(format!("- passed: {}", if review.passed { "yes" } else { "no" }));
            report_lines.push(format!("- summary: {}", review.summary));
            report_lines.push(String::new());
            let observations: Vec<serde_json::Value> =
                review.issues.iter().filter_map(|issue| review_issue_observation(issue)).collect();
            for issue in review.issues {
                report_lines.push(format!("- issue: {issue}"));
            }
            report_lines.push(String::new());
            reviewed = Some((review.summary, observations));
        }
        let translated_count = count_translated_segments(&source.segments, &completed_chapter.segments);
        manifest.updated_at = crate::utils::utc_time::utc_now_iso();
        for chapter in &mut manifest.chapters {
            if chapter.number == chapter_info.number {
                chapter.status = status;
                chapter.translated_segments = translated_count;
                // 968 号：manifest 章节标题=译后标题（TS updateChapterProgress）。
                chapter.title = completed_chapter.title.clone();
                let (summary, observations) = reviewed.take().unzip();
                chapter.review_summary = summary;
                chapter.observations = observations;
            }
        }
        save_translation_manifest(project_root, &manifest)
            .await
            .map_err(|e| e.to_string())?;
    }

    // 143 号：报告与 complete 快照同事务（TS commitProductionArtifacts——
    // artifacts = base + 全部章译文 + 报告）。
    let report_path = format!("translations/{project_id}/review-report.md");
    let mut artifacts = base_artifacts.to_vec();
    artifacts.extend(manifest.chapters.iter().map(|chapter| chapter.translated_path.clone()));
    artifacts.push(report_path.clone());
    commit_production_artifacts(
        project_root,
        vec![crate::utils::atomic_file_set::AtomicFileWrite {
            relative_path: report_path.clone(),
            content: crate::utils::atomic_file_set::FileContent::Text(format!(
                "{}\n",
                report_lines.join("\n").trim_end()
            )),
        }],
        run_path,
        &ProductionRunSnapshot::create(CreateRunInput {
            kind: ProductionKind::Translation,
            id: project_id.to_string(),
            status: ProductionRunStatus::Complete,
            stage: "complete".to_string(),
            artifacts,
            observations: Vec::new(),
            model: None,
            skill_ids: None,
            resume_cursor: None,
            error: None,
        }),
        Vec::new(),
        None,
    )
    .await
    .map_err(|e| e.to_string())?;
    // 967 号：TS 回执口径——total=源段总数、completed=已译段数（manifest 已逐章更新）。
    let total_segments: usize = manifest.chapters.iter().map(|chapter| chapter.segment_count).sum();
    let completed_segments: usize = manifest.chapters.iter().map(|chapter| chapter.translated_segments).sum();
    let pending_segments = total_segments.saturating_sub(completed_segments);
    // 968 号：回执 observations=manifest 全章 flatMap（TS 同口径，非仅本次运行）+
    // category/assessment/scope 缺省补齐（`chapter:N`）；targetHash/target 属 P4 工件面。
    let observations: Vec<serde_json::Value> = manifest
        .chapters
        .iter()
        .flat_map(|chapter| {
            chapter.observations.iter().flatten().map(move |observation| {
                let mut observation = observation.clone();
                if let Some(object) = observation.as_object_mut() {
                    object.entry("category").or_insert_with(|| serde_json::json!("quality"));
                    object.entry("assessment").or_insert_with(|| serde_json::json!("observation"));
                    object.entry("scope").or_insert_with(|| serde_json::json!(format!("chapter:{}", chapter.number)));
                }
                observation
            })
        })
        .collect();
    Ok(RunTranslationProjectResult {
        kind: if pending_segments > 0 { "translation_pending" } else { "translation_completed" },
        work_id: project_id.to_string(),
        project_id: project_id.to_string(),
        translated_segments,
        reviewed_chapters,
        report_path,
        total_segments,
        completed_segments,
        pending_segments,
        observations,
    })
}
