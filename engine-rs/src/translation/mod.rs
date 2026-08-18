//! 翻译域（文本处理）。
//!
//! 移植自 `packages/core/src/translation/text.ts`（89 行，纯函数）。
//! 依赖已移植的 [`crate::utils::split_chapters`]。
//!
//! ## 待移植（需 reqwest / llm）
//! translation/runner（翻译执行流）/ source（爬取源）/ epub/export（文件 IO）。

pub mod export;
pub mod llm_model;
pub mod project;
pub mod run_store;
pub mod runner;
pub mod source;
pub mod text;
pub mod types;

pub use text::{
    decode_html, normalize_translation_text, segment_translation_text_vec,
    split_translation_chapters, strip_html, TranslationTextChapter,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::types::*;
    use std::path::Path;

    fn input<'a>(file_path: &'a str) -> CreateTranslationProjectInput<'a> {
        CreateTranslationProjectInput {
            file_path,
            source_language: "en",
            target_language: "zh",
            title: None,
            segment_max_chars: None,
        }
    }

    #[tokio::test]
    async fn create_project_from_text_lays_out_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("novel.txt"), "Chapter 1\n\nFirst paragraph.\n\nSecond paragraph.\n\nChapter 2\n\nMore text here.").unwrap();

        let result = project::create_translation_project_from_file(root, &input("novel.txt"))
            .await
            .unwrap();
        assert!(result.project_dir.starts_with("translations/"), "{}", result.project_dir);
        assert_eq!(result.manifest.source_language, "en");
        assert_eq!(result.manifest.chapters.len(), 2);
        assert_eq!(result.manifest.chapters[0].status, TranslationChapterStatus::Pending);
        assert_eq!(result.manifest.source.kind, TranslationSourceKind::Text);
        // source/translated 章节文件 + manifest + glossary + review 报告。
        assert!(root.join("translations").join(&result.manifest.id).join("manifest.json").is_file());
        assert!(root.join(&result.manifest.chapters[0].source_path).is_file());
        assert!(root.join(&result.manifest.chapters[0].translated_path).is_file());
        assert!(root.join("translations").join(&result.manifest.id).join("glossary.json").is_file());
        assert!(root.join("translations").join(&result.manifest.id).join("review-report.md").is_file());
        // translated 章节空段起步。
        let translated: TranslationChapterFile = serde_json::from_str(
            &std::fs::read_to_string(root.join(&result.manifest.chapters[0].translated_path)).unwrap(),
        )
        .unwrap();
        assert!(translated.segments.is_empty());
        // source 章节分段。
        let source: TranslationChapterFile = serde_json::from_str(
            &std::fs::read_to_string(root.join(&result.manifest.chapters[0].source_path)).unwrap(),
        )
        .unwrap();
        assert_eq!(source.segments.len(), 2);
        assert_eq!(source.segments[0].index, 1);
    }

    #[tokio::test]
    async fn create_rejects_escape_and_missing_and_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("in.txt"), "hello").unwrap();

        let escape = input("../../etc/passwd");
        assert!(project::create_translation_project_from_file(root, &escape).await.is_err());
        let _ = &escape;

        let missing = input("ghost.txt");
        assert!(project::create_translation_project_from_file(root, &missing).await.is_err());

        std::fs::write(root.join("doc.pdf"), b"%PDF-fake").unwrap();
        let pdf = input("doc.pdf");
        let err = match project::create_translation_project_from_file(root, &pdf).await {
            Ok(_) => panic!("pdf 应被拒绝"),
            Err(message) => message,
        };
        assert!(err.contains("PDF"), "{err}");

        std::fs::write(root.join("pic.png"), b"\x89PNG").unwrap();
        let bad = input("pic.png");
        let err = match project::create_translation_project_from_file(root, &bad).await {
            Ok(_) => panic!("png 应被拒绝"),
            Err(message) => message,
        };
        assert!(err.contains("Unsupported"), "{err}");
    }

    #[test]
    fn epub_roundtrip_extract_and_export() {
        // 最小 epub：mimetype + container + opf(spine 2 章) + xhtml。
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write as _;
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let stored = zip::write::FileOptions::<()>::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated = zip::write::FileOptions::<()>::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("mimetype", stored).unwrap();
            writer.write_all(b"application/epub+zip").unwrap();
            writer.start_file("META-INF/container.xml", deflated).unwrap();
            writer.write_all(br#"<?xml version="1.0"?><container><rootfile full-path="OEBPS/content.opf"/></container>"#).unwrap();
            writer.start_file("OEBPS/content.opf", deflated).unwrap();
            writer.write_all("<?xml version=\"1.0\"?><package><metadata><dc:title>测试书</dc:title></metadata><manifest><item id=\"c1\" href=\"c1.xhtml\"/><item id=\"c2\" href=\"c2.xhtml\"/></manifest><spine><itemref idref=\"c1\"/><itemref idref=\"c2\"/></spine></package>".as_bytes()).unwrap();
            writer.start_file("OEBPS/c1.xhtml", deflated).unwrap();
            writer.write_all(b"<html><body><h1>Alpha</h1><p>First &amp; only.</p></body></html>").unwrap();
            writer.start_file("OEBPS/c2.xhtml", deflated).unwrap();
            writer.write_all(b"<html><body><h1>Beta</h1><p>Second chapter.</p></body></html>").unwrap();
            writer.finish().unwrap();
        }
        let extracted = source::extract_epub(&buffer.into_inner()).unwrap();
        assert_eq!(extracted.title.as_deref(), Some("测试书"));
        assert_eq!(extracted.chapters.len(), 2);
        assert_eq!(extracted.chapters[0].title, "Alpha");
        assert!(extracted.chapters[0].content.contains("First & only."));

        // 导出 epub：读回验证结构。
        let manifest = TranslationProjectManifest {
            id: "t".into(),
            title: "导出书".into(),
            source_language: "en".into(),
            target_language: "zh".into(),
            created_at: String::new(),
            updated_at: String::new(),
            source: TranslationSourceManifest {
                kind: TranslationSourceKind::Text,
                path: "in.txt".into(),
                char_count: 0,
                total_pages: None,
            },
            chapters: Vec::new(),
        };
        let chapters = vec![
            TranslationChapterFile {
                number: 1,
                title: "一章".into(),
                source_language: "en".into(),
                target_language: "zh".into(),
                segments: vec![TranslationSegment {
                    index: 1,
                    source: "hello".into(),
                    target: Some("你好 <世界>".into()),
                    notes: None,
                }],
            },
        ];
        let epub = export::tests_support::build_epub_for_test(&manifest, &chapters).unwrap();
        let mut reader = zip::ZipArchive::new(std::io::Cursor::new(&epub[..])).unwrap();
        assert_eq!(reader.by_name("mimetype").unwrap().compression(), zip::CompressionMethod::Stored);
        let mut content = String::new();
        {
            let mut archive = reader.clone();
            let mut file = archive.by_name("OEBPS/chapter-0001.xhtml").unwrap();
            std::io::Read::read_to_string(&mut file, &mut content).unwrap();
        }
        assert!(content.contains("你好 &lt;世界&gt;"), "XHTML 转义: {content}");
    }

    #[test]
    fn llm_json_parsing_tolerates_fences_and_substrings() {
        let model = llm_model::tests_support::noop();
        let _ = model;
        // fence 剥离 + 子串提取。
        let v = llm_model::tests_support::parse_json_object_pub(
            "```json\n{\"segments\": []}\n```",
        )
        .unwrap();
        assert!(v.get("segments").is_some());
        let v = llm_model::tests_support::parse_json_object_pub(
            "前置噪音 {\"passed\": true} 尾部",
        )
        .unwrap();
        assert_eq!(v.get("passed"), Some(&serde_json::json!(true)));
        assert!(llm_model::tests_support::parse_json_object_pub("no json").is_err());
    }

    #[test]
    fn slug_and_filename_safety() {
        // slug 折叠 + 默认回退。
        let s = project::tests_support::slug_pub("Hello, 世界! 2026");
        assert!(s.contains("hello"), "{s}");
        assert!(s.contains("世界"), "{s}");
        assert_eq!(project::tests_support::slug_pub("!!!"), "translation");
    }

    #[test]
    fn glossary_merge_dedupes_by_lowercase_source() {
        let terms = vec![
            TranslationGlossaryTerm { source: "Mana".into(), target: "魔力".into(), note: None },
            TranslationGlossaryTerm { source: "mana".into(), target: "法力".into(), note: Some("后写覆盖".into()) },
            TranslationGlossaryTerm { source: "  ".into(), target: "x".into(), note: None },
        ];
        let merged = run_store::merge_glossary_terms(&terms);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].target, "法力");
        assert_eq!(merged[0].note.as_deref(), Some("后写覆盖"));
    }

    /// 注入模型：全段回显 + 评审通过。
    struct EchoModel;

    #[async_trait::async_trait]
    impl TranslationModelPort for EchoModel {
        async fn translate_segments(
            &self,
            input: TranslateSegmentsInput<'_>,
        ) -> Result<TranslateSegmentsOutput, String> {
            Ok(TranslateSegmentsOutput {
                segments: input
                    .segments
                    .iter()
                    .map(|segment| TranslatedSegmentItem {
                        index: segment.index,
                        target: format!("[译]{}", segment.source),
                        notes: None,
                    })
                    .collect(),
                glossary: vec![TranslationGlossaryTerm {
                    source: "Mana".into(),
                    target: "魔力".into(),
                    note: None,
                }],
            })
        }

        async fn review_chapter(
            &self,
            _input: ReviewChapterInput<'_>,
        ) -> Result<ReviewChapterOutput, String> {
            Ok(ReviewChapterOutput {
                passed: true,
                summary: "Looks good.".into(),
                issues: Vec::new(),
            })
        }
    }


    // ── 143 号：翻译运行快照生命周期 ────────────────────────────────────

    #[tokio::test]
    async fn runner_publishes_run_snapshot_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("in2.txt"), "Chapter 1\n\nAlpha text.").unwrap();
        let created = project::create_translation_project_from_file(root, &input("in2.txt"))
            .await
            .unwrap();
        let id = created.manifest.id.clone();
        runner::run_translation_project(root, &id, &EchoModel, None)
            .await
            .unwrap();

        // 终态快照：complete / stage complete / artifacts = base + 章 + 报告。
        let run: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("translations").join(&id).join("status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(run["version"], 1);
        assert_eq!(run["kind"], "translation");
        assert_eq!(run["id"], id);
        assert_eq!(run["status"], "complete");
        assert_eq!(run["stage"], "complete");
        let artifacts: Vec<&str> = run["artifacts"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(artifacts.contains(&format!("translations/{id}/manifest.json").as_str()), "{artifacts:?}");
        assert!(artifacts.contains(&format!("translations/{id}/glossary.json").as_str()), "{artifacts:?}");
        assert!(artifacts.iter().any(|a| a.contains("/translated/")), "章译文在清单：{artifacts:?}");
        assert!(artifacts.contains(&format!("translations/{id}/review-report.md").as_str()), "{artifacts:?}");
        assert!(run.get("error").is_none());
    }

    #[tokio::test]
    async fn runner_failed_run_publishes_failed_snapshot() {
        struct FailingModel;
        #[async_trait::async_trait]
        impl TranslationModelPort for FailingModel {
            async fn translate_segments(
                &self,
                _input: TranslateSegmentsInput<'_>,
            ) -> Result<TranslateSegmentsOutput, String> {
                Err("model exploded".to_string())
            }
            async fn review_chapter(
                &self,
                _input: ReviewChapterInput<'_>,
            ) -> Result<ReviewChapterOutput, String> {
                unreachable!()
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("in3.txt"), "Chapter 1\n\nAlpha text.").unwrap();
        let created = project::create_translation_project_from_file(root, &input("in3.txt"))
            .await
            .unwrap();
        let id = created.manifest.id.clone();
        let err = runner::run_translation_project(root, &id, &FailingModel, None)
            .await
            .expect_err("模型失败应上抛");
        assert!(err.contains("model exploded"), "{err}");
        // failed 快照落地（吞写错不覆盖原错）。
        let run: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("translations").join(&id).join("status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(run["status"], "failed");
        assert_eq!(run["stage"], "translate");
        assert_eq!(run["error"], "model exploded");
    }

    #[tokio::test]
    async fn runner_translates_reviews_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let root: &Path = dir.path();
        std::fs::write(root.join("in.txt"), "Chapter 1\n\nAlpha text.").unwrap();
        let created = project::create_translation_project_from_file(root, &input("in.txt"))
            .await
            .unwrap();
        let id = created.manifest.id.clone();

        let result = runner::run_translation_project(root, &id, &EchoModel, None)
            .await
            .unwrap();
        assert_eq!(result.translated_segments, 1);
        assert_eq!(result.reviewed_chapters, 1);
        assert_eq!(result.report_path, format!("translations/{id}/review-report.md"));

        // 状态推进 + 报告内容 + 术语表。
        let manifest = run_store::load_translation_manifest(root, &id).await.unwrap();
        assert_eq!(manifest.chapters[0].status, TranslationChapterStatus::Reviewed);
        let report = std::fs::read_to_string(root.join("translations").join(&id).join("review-report.md")).unwrap();
        assert!(report.contains("- passed: yes"), "{report}");
        let glossary = run_store::load_translation_glossary(root, &id).await;
        assert_eq!(glossary.len(), 1);
        assert_eq!(glossary[0].target, "魔力");
    }

    #[tokio::test]
    async fn export_md_and_txt_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("in.txt"), "Chapter 1\n\nAlpha.").unwrap();
        let created = project::create_translation_project_from_file(root, &input("in.txt"))
            .await
            .unwrap();
        let id = created.manifest.id.clone();
        runner::run_translation_project(root, &id, &EchoModel, None)
            .await
            .unwrap();

        let md = export::write_translation_export(root, &id, TranslationExportFormat::Md, None)
            .await
            .unwrap();
        assert!(md.output_path.starts_with("translations/"), "{}", md.output_path);
        assert!(md.output_path.ends_with(".md"));
        let text = std::fs::read_to_string(root.join(&md.output_path)).unwrap();
        assert!(text.starts_with("# in"), "{text}");
        assert!(text.contains("> en -> zh"), "{text}");
        assert!(text.contains("## Chapter 1"), "{text}");
        assert!(text.contains("[译]Alpha."), "{text}");

        let txt = export::write_translation_export(root, &id, TranslationExportFormat::Txt, None)
            .await
            .unwrap();
        let text = std::fs::read_to_string(root.join(&txt.output_path)).unwrap();
        assert!(!text.contains("# "), "{text}");
        assert!(text.contains("en -> zh"), "{text}");
    }
}
