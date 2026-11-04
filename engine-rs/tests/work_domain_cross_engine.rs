//! 跨引擎 work 域金标准（882 号 M1a）——与 TS 侧
//! `packages/core/src/__tests__/work-store-cross-engine.test.ts` **逐字共用**
//! 同一组金标准字符串（程序化核验逐字一致）：双引擎（Rust 引擎 ↔ Node 回退端）
//! 交替读写同一项目的 works/<id>/ 与 harness.sqlite，schema 漂移即两侧必红。
//!
//! 覆盖：work.json 双版本 artifact 不变量（r1 superseded + r2 current）、
//! episode+events 的 JSON 出参形状、WorkProfile 的 2.0 parse 全量形状。

use inkos_engine::harness::episode_store::{CreativeEpisodeStore, ListEpisodesOptions};
use inkos_engine::harness::{
    create_work_manifest, list_work_manifests, load_work_manifest, parse_work_manifest_json,
    promote_artifact_revision, save_work_manifest, stage_artifact_revision, work_manifest_path,
    ArtifactRevisionStatus, CreateWorkManifestInput, StageArtifactRevisionInput,
    WorkProfile,
};

const GOLDEN_WORK_MANIFEST_JSON: &str = r#"{
  "version": 2,
  "id": "walk-diff",
  "title": "雨夜碎镜（跨引擎金标准）",
  "profileId": "short-fiction",
  "language": "zh",
  "status": "active",
  "lineage": [],
  "artifacts": [
    {
      "id": "outline",
      "kind": "outline",
      "currentRevisionId": "r2",
      "revisions": [
        {
          "id": "r1",
          "parentRevisionId": null,
          "path": "artifacts/outline/revisions/r1.md",
          "contentType": "text/markdown",
          "status": "superseded",
          "checksum": "sha256:70c4577c024de823b62a833caf7733f07e729921c6b49875db8b1602403e7331",
          "byteLength": 68,
          "createdAt": "2026-11-02T00:00:00.000Z"
        },
        {
          "id": "r2",
          "parentRevisionId": "r1",
          "path": "artifacts/outline/revisions/r2.md",
          "contentType": "text/markdown",
          "status": "current",
          "checksum": "sha256:f0d2e9e9414e33c22c84474ba8902291818bf01a4089bbc5245315d2c16782d6",
          "byteLength": 107,
          "createdAt": "2026-11-03T00:00:00.000Z"
        }
      ],
      "metadata": {}
    }
  ],
  "metadata": {},
  "createdAt": "2026-11-02T00:00:00.000Z",
  "updatedAt": "2026-11-03T00:00:00.000Z"
}"#;

const GOLDEN_REV2_TEXT: &str = "# 短篇大纲 v2\n\n雨夜来客叩门；斋主窥见镜中人影与自己同貌，交易自此刻开始。\n";

/// 金标准 profile（与 builtin-profiles.ts 的 short-fiction WorkProfileSchema
/// parse 输出一致：requiredSkillIds/recommendedSkillIds 来自 parse 补全）。
const GOLDEN_PROFILE_JSON: &str = r#"{
  "version": 2,
  "id": "short-fiction",
  "title": "Short fiction",
  "description": "",
  "capabilityIds": ["workspace", "short-fiction", "adaptation", "visual"],
  "requiredSkillIds": ["inkos-short-writing"],
  "recommendedSkillIds": [],
  "artifactKinds": ["outline", "manuscript", "sales-package", "cover"],
  "confirmation": {
    "inferredMutation": "execute",
    "explicitRecoverableMutation": "execute",
    "destructiveMutation": "confirm"
  },
  "artifactSchemas": {
    "source/final/short-story.json": "short-manuscript",
    "source/final/sales-package.json": "short-package"
  },
  "qualityCriteria": [],
  "production": {}
}"#;

#[test]
fn golden_profile_round_trips() {
    let profile: WorkProfile = serde_json::from_str(GOLDEN_PROFILE_JSON).expect("金标准 profile 可解析");
    assert_eq!(profile.id, "short-fiction");
    assert_eq!(profile.capability_ids.len(), 4);
    assert_eq!(profile.production.min_chapter_length_ratio, None, "production 默认空对象");
    profile.validate().unwrap();
    // 往返：序列化后再次解析等值
    let written = serde_json::to_value(&profile).unwrap();
    let reparsed: WorkProfile = serde_json::from_value(written).unwrap();
    assert_eq!(reparsed, profile);
}

#[tokio::test]
async fn golden_manifest_round_trips_through_parse_and_save_load() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let manifest = parse_work_manifest_json(GOLDEN_WORK_MANIFEST_JSON).unwrap();
    save_work_manifest(root, &manifest).await.unwrap();
    // 落盘文件逐字节等于金标准（尾部换行对齐 TS JSON.stringify(…, null, 2)+"\n"）
    let written = std::fs::read_to_string(work_manifest_path(root, "walk-diff").unwrap()).unwrap();
    let expected = format!("{GOLDEN_WORK_MANIFEST_JSON}\n");
    assert_eq!(written, expected, "落盘文件须与金标准逐字一致");
    let loaded = load_work_manifest(root, "walk-diff").await.unwrap();
    assert_eq!(loaded, manifest);
}

#[tokio::test]
async fn golden_manifest_survives_stage_new_revision_with_correct_checksums() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let manifest = parse_work_manifest_json(GOLDEN_WORK_MANIFEST_JSON).unwrap();
    save_work_manifest(root, &manifest).await.unwrap();
    let staged = stage_artifact_revision(&StageArtifactRevisionInput {
        project_root: root,
        manifest: &manifest,
        artifact_id: "outline",
        artifact_kind: "outline",
        revision_id: "r3",
        content: GOLDEN_REV2_TEXT,
        content_type: "text/markdown",
        file_name: "r3.md",
        episode_id: None,
        now: "2026-11-04T00:00:00.000Z",
        created_at: Some("2026-11-04T00:00:00.000Z"),
        promote: false,
    })
    .await
    .unwrap();
    // candidate 不改变 current
    assert_eq!(staged.revision.status, ArtifactRevisionStatus::Candidate);
    assert_eq!(staged.manifest.artifacts[0].current_revision_id.as_deref(), Some("r2"));
    assert_eq!(
        staged.revision.checksum,
        format!(
            "sha256:{}",
            {
                use sha2::{Digest, Sha256};
                let mut h = Sha256::new();
                h.update(GOLDEN_REV2_TEXT.as_bytes());
                format!("{:x}", h.finalize())
            }
        )
    );
    // promote 后 current 翻转
    let promoted = promote_artifact_revision(root, &staged.manifest, "outline", "r3", "2026-11-04T01:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(promoted.artifacts[0].current_revision_id.as_deref(), Some("r3"));
    let r2 = promoted.artifacts[0]
        .revisions
        .iter()
        .find(|revision| revision.id == "r2")
        .unwrap();
    assert_eq!(r2.status, ArtifactRevisionStatus::Superseded);
}

#[tokio::test]
async fn create_work_manifest_produces_empty_draft_and_list_sorts_by_updated_at() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let draft = create_work_manifest(&CreateWorkManifestInput {
        id: "draft-1",
        title: "草稿",
        profile_id: "short-fiction",
        language: "zh",
        status: None,
        now: "2026-11-01T00:00:00.000Z",
    })
    .unwrap();
    save_work_manifest(root, &draft).await.unwrap();
    let mut later = parse_work_manifest_json(GOLDEN_WORK_MANIFEST_JSON).unwrap();
    later.updated_at = "2026-11-05T00:00:00.000Z".into();
    save_work_manifest(root, &later).await.unwrap();
    let list = list_work_manifests(root, None).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, "walk-diff", "updatedAt 降序");
    // draft-1 与 walk-diff 同为 short-fiction → 过滤结果为 2（过滤语义验证）
    let shorts = list_work_manifests(root, Some("short-fiction")).await.unwrap();
    assert_eq!(shorts.len(), 2);
    assert_eq!(shorts[0].id, "walk-diff");
}

#[test]
fn malformed_manifest_is_rejected_strictly() {
    // 未知字段（zod strict 语义）→ 拒
    let bad = GOLDEN_WORK_MANIFEST_JSON.replace("\"lineage\": []", "\"lineage\": [], \"sneaky\": 1");
    assert!(parse_work_manifest_json(&bad).is_err());
    // currentRevisionId 指向不存在的 revision → superRefine 拒
    let bad = GOLDEN_WORK_MANIFEST_JSON.replace("\"currentRevisionId\": \"r2\"", "\"currentRevisionId\": \"ghost\"");
    assert!(parse_work_manifest_json(&bad).is_err());
}

#[test]
fn episode_store_cross_engine_golden_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = CreativeEpisodeStore::new(&dir.path().join("harness.sqlite")).unwrap();
    // 模拟 Node 回退端写入的行（CreativeEpisodeStore.create/append 的 JSON 出参形状）
    let episode = serde_json::from_str::<inkos_engine::harness::CreativeEpisode>(
        r#"{"version":2,"id":"ep-walkdiff-1","workId":"walk-diff","profileId":"short-fiction","status":"running","startedAt":"2026-11-03T00:00:00.000Z","completedAt":null}"#,
    )
    .unwrap();
    store.create(&episode).unwrap();
    store
        .append("ep-walkdiff-1", "stage.completed", Some("walk-diff"), Some("short-fiction"), Some("outline-draft"), &serde_json::json!({"stage": "outline"}), "2026-11-03T00:00:30.000Z")
        .unwrap();
    store
        .append("ep-walkdiff-1", "review.completed", Some("walk-diff"), Some("short-fiction"), None, &serde_json::json!({"score": 91}), "2026-11-03T00:00:50.000Z")
        .unwrap();
    store
        .finish(
            "ep-walkdiff-1",
            inkos_engine::harness::EpisodeStatus::Completed,
            "2026-11-03T00:01:00.000Z",
        )
        .unwrap();

    let detail = store.require_episode("ep-walkdiff-1").unwrap();
    assert_eq!(detail.status, inkos_engine::harness::EpisodeStatus::Completed);
    let events = store.list_events("ep-walkdiff-1").unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "stage.completed");
    assert_eq!(events[1].seq, 1);

    // 按 workId 过滤（listEpisodes 的 EXISTS 分支）
    let listed = store
        .list_episodes(&ListEpisodesOptions {
            work_id: Some("walk-diff".into()),
            profile_id: None,
            status: None,
            limit: Some(50),
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    store.close();
}

#[test]
fn unknown_work_id_is_rejected_by_store_layer() {
    let error = create_work_manifest(&CreateWorkManifestInput {
        id: "../escape",
        title: "越狱",
        profile_id: "short-fiction",
        language: "zh",
        status: None,
        now: "2026-11-01T00:00:00.000Z",
    })
    .unwrap_err();
    assert!(matches!(error, inkos_engine::harness::WorkStoreError::Invalid(_)));
}
