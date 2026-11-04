//! work 域 M1b 端点活体测试（884 号）：works/episodes/profiles GET 全域 + adopt
//! 最小环 + skills documents GET——`packages/studio/src/api/server.ts` 对应端点的
//! 双引擎契约面（L1 读写面，881 设计 §五）。
//!
//! 错误面契约逐端点对齐（勿"修复"）：
//! - works GET 域局部 catch：ENOENT→404、其余→400，`{error:"<message>"}` 纯字符串
//! - /episodes/:id、/profiles、adopt 无局部 catch → 500 INTERNAL_ERROR 固定文案
//! - revisions 端点 checksum 不符 = 400（TS ApiError(409) 被局部 catch 降级，同款）
//!
//! 与 TS 侧 `packages/core/src/__tests__/work-store-cross-engine.test.ts` 逐字共用
//! profile 金标准（双端恒等锁，882 缺口清偿 + M1b 扩展）。

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

use inkos_engine::harness::episode_store::{CreativeEpisodeStore, ListEpisodesOptions};
use inkos_engine::harness::source_sync::hex_sha256;
use inkos_engine::harness::work_store::{
    manifest_to_json, parse_work_manifest_json, work_manifest_path,
};

// ── 双端共用 profile 金标准（与 TS 测试逐字一致）─────────────────────

const GOLDEN_SHORT_FICTION_PROFILE_JSON: &str = r#"{
  "version": 2,
  "id": "short-fiction",
  "title": "Short fiction",
  "description": "",
  "capabilityIds": [
    "workspace",
    "short-fiction",
    "adaptation",
    "visual"
  ],
  "requiredSkillIds": [
    "inkos-short-writing"
  ],
  "recommendedSkillIds": [],
  "artifactKinds": [
    "outline",
    "manuscript",
    "sales-package",
    "cover"
  ],
  "confirmation": {
    "inferredMutation": "execute",
    "explicitRecoverableMutation": "execute",
    "destructiveMutation": "confirm"
  },
  "contextRecipe": {
    "id": "short-fiction",
    "sourceIds": [
      "task",
      "skills",
      "work"
    ]
  },
  "artifactSchemas": {
    "source/final/short-story.json": "short-manuscript",
    "source/final/sales-package.json": "short-package"
  },
  "qualityCriteria": [],
  "production": {}
}"#;

const GOLDEN_WORKSPACE_DEFAULT_PROFILE_JSON: &str = r#"{
  "version": 2,
  "id": "workspace-default",
  "title": "Creative workspace",
  "description": "Discuss goals, inspect works, and create derived works.",
  "capabilityIds": [
    "workspace",
    "adaptation",
    "translation",
    "visual"
  ],
  "requiredSkillIds": [],
  "recommendedSkillIds": [],
  "artifactKinds": [
    "brief",
    "source",
    "export"
  ],
  "confirmation": {
    "inferredMutation": "execute",
    "explicitRecoverableMutation": "execute",
    "destructiveMutation": "confirm"
  },
  "contextRecipe": {
    "id": "workspace-default",
    "sourceIds": [
      "task",
      "skills",
      "work"
    ]
  },
  "artifactSchemas": {},
  "qualityCriteria": [],
  "production": {}
}"#;

// ── 路由装配（e2e_read_chain_zero_write 同款生产形态）─────────────────

fn assemble(root: &Path) -> axum::Router {
    let hub = Arc::new(inkos_engine::server::sse::BroadcastHub::new());
    let state = Arc::new(inkos_engine::state::manager::StateManager::new(
        root.display().to_string().as_str(),
    ));
    let agent_router = Arc::new(inkos_engine::llm::agent_router::AgentRouter::new(
        inkos_engine::llm::agent_router::LlmEndpointConfig {
            context_window_tokens: 128_000,
            base_url: "http://127.0.0.1:9".into(),
            api_key: "k".into(),
            model: "m".into(),
            max_tokens: 1024,
            extra_headers: Default::default(),
        },
        Default::default(),
    ));
    let runner: inkos_engine::server::write_next_route::WriteNextRunner =
        Arc::new(|_state, _book, _target, _quality, _abort, _mode| {
            Box::pin(async {
                Err::<inkos_engine::pipeline::write_next::ChapterPipelineResult, _>(
                    "M1b 契约测试不触写链".to_string(),
                )
            })
        });
    let books = inkos_engine::server::books_routes::BooksRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: agent_router.clone(),
        builtin_genres_dir: root.to_path_buf(),
        revision_gate: inkos_engine::pipeline::merged_audit::RevisionGate::default(),
    };
    let audit = inkos_engine::server::audit_route::AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: agent_router,
        builtin_genres_dir: root.to_path_buf(),
    };
    let write_next = inkos_engine::server::write_next_route::WriteNextRuntime {
        hub: hub.clone(),
        state,
        runner,
        project_root: root.to_path_buf(),
    };
    inkos_engine::server::router_books(
        inkos_engine::server::AppState { version: "work-m1b".into() },
        hub,
        write_next,
        audit,
        books,
    )
}

async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Option<String>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(path);
    let request = match body {
        Some(payload) => builder
            .header("content-type", "application/json")
            .body(Body::from(payload))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

// ── 种子 ─────────────────────────────────────────────────────────────

const REV1_TEXT: &str = "# 大纲 v1\n\n雨夜来客。\n";
const REV2_TEXT: &str = "# 大纲 v2\n\n雨夜来客叩门；镜中人影与来客同貌。\n";

fn sha256_hex(text: &str) -> String {
    hex_sha256(text.as_bytes())
}

/// 种子 work：outline 工件，r1=current（源文件在位），r2=candidate（快照在位）。
/// `revision_path` 决定修订路径（source/… = adopt 可用；artifacts/… = DERIVED 变体）。
fn seed_adopt_work(root: &Path, work_id: &str, profile_id: &str, revision_path: &str) {
    let rev1_checksum = format!("sha256:{}", sha256_hex(REV1_TEXT));
    let rev2_checksum = format!("sha256:{}", sha256_hex(REV2_TEXT));
    let manifest = format!(
        r#"{{
  "version": 2,
  "id": "{work_id}",
  "title": "采用环金标准",
  "profileId": "{profile_id}",
  "language": "zh",
  "status": "active",
  "lineage": [],
  "artifacts": [
    {{
      "id": "outline",
      "kind": "file",
      "currentRevisionId": "r1",
      "revisions": [
        {{
          "id": "r1",
          "parentRevisionId": null,
          "path": "{revision_path}",
          "snapshotPath": "revisions/outline/r1.md",
          "contentType": "text/markdown",
          "status": "current",
          "checksum": "{rev1_checksum}",
          "byteLength": {rev1_len},
          "createdAt": "2026-11-01T00:00:00.000Z"
        }},
        {{
          "id": "r2",
          "parentRevisionId": "r1",
          "path": "{revision_path}",
          "snapshotPath": "revisions/outline/r2.md",
          "contentType": "text/markdown",
          "status": "candidate",
          "checksum": "{rev2_checksum}",
          "byteLength": {rev2_len},
          "episodeId": "ep-seed",
          "createdAt": "2026-11-02T00:00:00.000Z"
        }}
      ],
      "metadata": {{"sourcePath": "works/{work_id}/source/outline.md", "kindSource": "storage-path-v1"}}
    }}
  ],
  "metadata": {{}},
  "createdAt": "2026-11-01T00:00:00.000Z",
  "updatedAt": "2026-11-02T00:00:00.000Z"
}}"#,
        work_id = work_id,
        profile_id = profile_id,
        revision_path = revision_path,
        rev1_checksum = rev1_checksum,
        rev1_len = REV1_TEXT.len(),
        rev2_checksum = rev2_checksum,
        rev2_len = REV2_TEXT.len(),
    );
    let manifest = parse_work_manifest_json(&manifest).expect("种子清单合法");
    let work_dir = root.join("works").join(work_id);
    // 源文件在位（r1 内容）+ 两份快照；manifest_to_json = 结构体序落盘（M1a 同源）。
    if let Some(parent) = work_dir.join(revision_path).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::create_dir_all(work_dir.join("revisions").join("outline")).unwrap();
    std::fs::write(work_dir.join(revision_path), REV1_TEXT).unwrap();
    std::fs::write(work_dir.join("revisions").join("outline").join("r1.md"), REV1_TEXT).unwrap();
    std::fs::write(work_dir.join("revisions").join("outline").join("r2.md"), REV2_TEXT).unwrap();
    std::fs::write(work_manifest_path(root, work_id).unwrap(), manifest_to_json(&manifest).unwrap())
        .unwrap();
}

// ── profile 金标准 + 端点 ────────────────────────────────────────────

#[test]
fn golden_builtin_profiles_match_ts_shared_strings() {
    use inkos_engine::harness::profile_registry::built_in_work_profiles;
    let profiles = built_in_work_profiles();
    assert_eq!(profiles.len(), 9, "内置 9 个 profile");
    // 契约语义比对（对象键序无 JSON 语义）：serde Value Map = BTreeMap 键序
    //（嵌套 artifactSchemas 字母序）vs TS JSON.stringify 插入序——字段集与值
    // 必须全等；顶层字段序由结构体声明序保证（与 zod schema 序一致）。TS 侧
    // 同字符串逐字节锁（work-store-cross-engine.test.ts）。
    let golden_short: Value = serde_json::from_str(GOLDEN_SHORT_FICTION_PROFILE_JSON).unwrap();
    let golden_workspace: Value = serde_json::from_str(GOLDEN_WORKSPACE_DEFAULT_PROFILE_JSON).unwrap();
    let short_fiction = profiles.iter().find(|profile| profile.id == "short-fiction").unwrap();
    assert_eq!(
        serde_json::to_value(short_fiction).unwrap(),
        golden_short,
        "short-fiction 金标准全字段全值一致（与 TS 测试共用字符串）"
    );
    let workspace = profiles.iter().find(|profile| profile.id == "workspace-default").unwrap();
    assert_eq!(
        serde_json::to_value(workspace).unwrap(),
        golden_workspace,
        "workspace-default 金标准全字段全值一致（与 TS 测试共用字符串）"
    );
}

#[tokio::test]
async fn profiles_endpoint_lists_nine_sorted_and_merges_custom() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let app = assemble(root);
    let (status, body) = request(&app, "GET", "/api/v1/profiles", None).await;
    assert_eq!(status, StatusCode::OK);
    let profiles = body["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 9);
    // list 按 id 升序（TS localeCompare）
    let ids: Vec<&str> = profiles.iter().map(|profile| profile["id"].as_str().unwrap()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    // 全字段出参（zod parse 后 default 已应用——artifactSchemas {} 等在场）
    assert!(profiles.iter().all(|profile| profile["qualityCriteria"].is_array()));
    assert!(profiles.iter().all(|profile| profile["artifactSchemas"].is_object()));
    assert!(profiles.iter().all(|profile| profile["confirmation"]["destructiveMutation"] == "confirm"));
    assert!(profiles.iter().all(|profile| profile["contextRecipe"]["sourceIds"][0] == "task"));

    // 自定义归并：新增 + 同名覆盖内置
    std::fs::create_dir_all(root.join(".inkos").join("profiles")).unwrap();
    std::fs::write(
        root.join(".inkos").join("profiles").join("zz-custom.json"),
        r#"{"version":2,"id":"zz-custom","title":"自定义","description":"","capabilityIds":["workspace"],"requiredSkillIds":[],"recommendedSkillIds":[],"artifactKinds":[],"confirmation":{"inferredMutation":"execute","explicitRecoverableMutation":"execute","destructiveMutation":"confirm"},"artifactSchemas":{},"qualityCriteria":[],"production":{}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".inkos").join("profiles").join("short-fiction.json"),
        GOLDEN_SHORT_FICTION_PROFILE_JSON.replace("\"title\": \"Short fiction\"", "\"title\": \"短篇（项目覆盖）\""),
    )
    .unwrap();
    let (status, body) = request(&app, "GET", "/api/v1/profiles", None).await;
    assert_eq!(status, StatusCode::OK);
    let profiles = body["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 10, "新增 1 + 覆盖 0（replace 语义）");
    let overridden = profiles.iter().find(|profile| profile["id"] == "short-fiction").unwrap();
    assert_eq!(overridden["title"], "短篇（项目覆盖）");
    assert_eq!(overridden["artifactSchemas"]["source/final/short-story.json"], "short-manuscript");
    assert!(profiles.iter().any(|profile| profile["id"] == "zz-custom"));

    // 非法自定义 profile（version≠2）→ 500 INTERNAL_ERROR 固定文案
    std::fs::write(
        root.join(".inkos").join("profiles").join("bad.json"),
        r#"{"version":3,"id":"bad","title":"坏","description":"","capabilityIds":["workspace"],"requiredSkillIds":[],"recommendedSkillIds":[],"artifactKinds":[],"confirmation":{"inferredMutation":"execute","explicitRecoverableMutation":"execute","destructiveMutation":"confirm"},"artifactSchemas":{},"qualityCriteria":[],"production":{}}"#,
    )
    .unwrap();
    let (status, body) = request(&app, "GET", "/api/v1/profiles", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "INTERNAL_ERROR");
    assert_eq!(body["error"]["message"], "Unexpected server error.");
}

// ── works GET 域 ─────────────────────────────────────────────────────

#[tokio::test]
async fn works_endpoints_shapes_and_error_faces() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_adopt_work(root, "adopt-work", "workspace-default", "source/outline.md");
    let app = assemble(root);

    // GET /works：artifactCount + updatedAt 降序
    let (status, body) = request(&app, "GET", "/api/v1/works", None).await;
    assert_eq!(status, StatusCode::OK);
    let works = body["works"].as_array().unwrap();
    assert_eq!(works.len(), 1);
    assert_eq!(works[0]["id"], "adopt-work");
    assert_eq!(works[0]["artifactCount"], 1);
    assert_eq!(works[0]["profileId"], "workspace-default");

    // profileId 过滤：命中与不命中
    let (status, body) =
        request(&app, "GET", "/api/v1/works?profileId=workspace-default", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["works"].as_array().unwrap().len(), 1);
    let (status, body) = request(&app, "GET", "/api/v1/works?profileId=short-fiction", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["works"].as_array().unwrap().len(), 0);

    // GET /works/:id：work + episodes（空库 = []）
    let (status, body) = request(&app, "GET", "/api/v1/works/adopt-work", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["work"]["id"], "adopt-work");
    assert_eq!(body["episodes"].as_array().unwrap().len(), 0);

    // GET /works/:id/artifacts
    let (status, body) = request(&app, "GET", "/api/v1/works/adopt-work/artifacts", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["workId"], "adopt-work");
    let artifacts = body["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["id"], "outline");
    assert_eq!(artifacts[0]["revisions"].as_array().unwrap().len(), 2);

    // 错误面：未知 work → 404 纯字符串 error（局部 catch）
    let (status, body) = request(&app, "GET", "/api/v1/works/ghost", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string(), "局部 catch = 纯字符串 error：{}", body);

    // 变异③ 未知字段：strict 拒 → 400 纯字符串
    let work_dir = root.join("works").join("adopt-work");
    let original = std::fs::read_to_string(work_dir.join("work.json")).unwrap();
    std::fs::write(
        work_dir.join("work.json"),
        original.replace("\"lineage\": [", "\"sneaky\": 1, \"lineage\": ["),
    )
    .unwrap();
    let (status, body) = request(&app, "GET", "/api/v1/works/adopt-work", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "strict 未知键 → 400：{}", body);
    assert!(body["error"].is_string());
    std::fs::write(work_dir.join("work.json"), original).unwrap();
}

// ── revisions GET ────────────────────────────────────────────────────

#[tokio::test]
async fn revision_endpoint_content_download_and_checksum_face() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_adopt_work(root, "adopt-work", "workspace-default", "source/outline.md");
    let app = assemble(root);

    // markdown → content
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/r2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["content"], REV2_TEXT);
    assert_eq!(body["revision"]["id"], "r2");
    assert_eq!(body["revision"]["status"], "candidate");
    assert_eq!(body["artifactId"], "outline");

    // download=1 → 附件字节
    let download_request = Request::builder()
        .method("GET")
        .uri("/api/v1/works/adopt-work/artifacts/outline/revisions/r2?download=1")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(download_request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let disposition = response
        .headers()
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .unwrap()
        .to_string();
    assert!(disposition.starts_with("attachment; filename=\""), "{}", disposition);
    assert!(disposition.contains("filename*=UTF-8''"), "{}", disposition);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(bytes.as_ref(), REV2_TEXT.as_bytes());

    // 变异① checksum 不符 → 400 固定文案（TS ApiError(409) 被局部 catch 降级）
    let snapshot = root
        .join("works")
        .join("adopt-work")
        .join("revisions")
        .join("outline")
        .join("r2.md");
    std::fs::write(&snapshot, "被篡改的内容\n").unwrap();
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/r2",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "checksum 不符 → 400（非 409，TS 局部 catch 同款降级）"
    );
    assert_eq!(body["error"], "Revision content does not match its recorded checksum");
    std::fs::write(&snapshot, REV2_TEXT).unwrap();

    // 快照缺失 → 404
    std::fs::remove_file(&snapshot).unwrap();
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/r2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].is_string());

    // 未知修订 → 404 固定文案
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/ghost",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "Artifact revision not found");
}

// ── adopt 环 ─────────────────────────────────────────────────────────

fn adopt_body(expected: &str) -> Option<String> {
    Some(serde_json::json!({ "expectedCurrentRevisionId": expected }).to_string())
}

#[tokio::test]
async fn adopt_happy_path_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_adopt_work(root, "adopt-work", "workspace-default", "source/outline.md");
    let app = assemble(root);

    let expected_revision = format!("rev-{}", &sha256_hex(REV2_TEXT)[..16]);
    let (status, body) = request(
        &app,
        "POST",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/r2/adopt",
        adopt_body("r1"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    let action = &body["action"];
    assert_eq!(action["status"], "success");
    assert_eq!(action["summary"], "Adopt work revision");
    assert_eq!(action["content"], "Adopted revision r2.");
    let artifacts = action["artifacts"].as_array().unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["artifactId"], "outline");
    assert_eq!(artifacts[0]["revisionId"], expected_revision);
    assert_eq!(artifacts[0]["path"], "source/outline.md");
    assert_eq!(action["observations"], Value::Array(vec![]));
    assert_eq!(action["data"]["kind"], "work_revision_adopted");
    assert_eq!(action["data"]["previousRevisionId"], "r1");

    // 清单翻转：current = rev-<checksum16>；r1 superseded；r2 candidate 保留
    let manifest_text =
        std::fs::read_to_string(work_manifest_path(root, "adopt-work").unwrap()).unwrap();
    let manifest = parse_work_manifest_json(&manifest_text).unwrap();
    let artifact = &manifest.artifacts[0];
    assert_eq!(artifact.current_revision_id.as_deref(), Some(expected_revision.as_str()));
    let statuses: Vec<(&str, String)> = artifact
        .revisions
        .iter()
        .map(|revision| (revision.id.as_str(), format!("{:?}", revision.status)))
        .collect();
    assert!(statuses.contains(&("r1", "Superseded".to_string())), "{statuses:?}");
    assert!(statuses.contains(&("r2", "Candidate".to_string())), "{statuses:?}");
    assert!(statuses.contains(&(expected_revision.as_str(), "Current".to_string())), "{statuses:?}");

    // 源文件与快照落位
    let source = std::fs::read_to_string(
        root.join("works").join("adopt-work").join("source").join("outline.md"),
    )
    .unwrap();
    assert_eq!(source, REV2_TEXT);
    let snapshot = std::fs::read_to_string(
        root.join("works")
            .join("adopt-work")
            .join("revisions")
            .join("outline")
            .join(format!("{expected_revision}.md")),
    )
    .unwrap();
    assert_eq!(snapshot, REV2_TEXT);

    // episode 事件序列：started → action-started → work-artifacts-synced →
    // action-completed → episode-completed；episode 行 completed
    let store = CreativeEpisodeStore::new(&root.join(".inkos").join("harness.sqlite")).unwrap();
    let listed = store
        .list_episodes(&ListEpisodesOptions {
            work_id: Some("adopt-work".into()),
            profile_id: None,
            status: None,
            limit: Some(50),
        })
        .unwrap();
    assert_eq!(listed.len(), 1);
    let episode = &listed[0];
    assert_eq!(episode.status, inkos_engine::harness::EpisodeStatus::Completed);
    assert_eq!(episode.profile_id.as_deref(), Some("workspace-default"));
    let events = store.list_events(&episode.id).unwrap();
    let types: Vec<&str> = events.iter().map(|event| event.event_type.as_str()).collect();
    assert_eq!(
        types,
        vec![
            "episode-started",
            "action-started",
            "work-artifacts-synced",
            "action-completed",
            "episode-completed"
        ],
        "事件序列：{types:?}"
    );
    assert_eq!(events[0].payload["profileId"], "workspace-default");
    assert_eq!(events[0].payload["baseline"][0]["artifactId"], "outline");
    assert_eq!(events[1].payload["risk"], "recoverable-write");
    assert_eq!(events[1].payload["source"], "explicit");
    assert_eq!(events[1].payload["parameters"]["expectedCurrentRevisionId"], "r1");
    assert_eq!(events[2].payload["workId"], "adopt-work");
    assert_eq!(events[2].payload["accepted"], true);
    assert!(events[2].payload["actionExecutionId"].is_string());
    assert_eq!(events[3].payload["status"], "success");
    assert_eq!(events[3].payload["artifactCount"], 1);
    store.close();
}

#[tokio::test]
async fn adopt_failures_match_global_error_face_and_record_events() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_adopt_work(root, "adopt-work", "workspace-default", "source/outline.md");
    let app = assemble(root);

    // ① 未知修订 → 500 INTERNAL_ERROR 固定文案（TS 无局部 catch → onError）
    let (status, body) = request(
        &app,
        "POST",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/ghost/adopt",
        adopt_body("r1"),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "INTERNAL_ERROR");
    assert_eq!(body["error"]["message"], "Unexpected server error.");

    // ② 变异② currentRevisionId 漂移：expected 不符 → 500 + action-failed 事件
    //    携带 code（ARTIFACT_REVISION_CONFLICT）
    let (status, _) = request(
        &app,
        "POST",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/r2/adopt",
        adopt_body("stale"),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // ③ 非 source/ 路径（走查夹具形态）→ ARTIFACT_DERIVED：重写清单修订路径
    let manifest_path = work_manifest_path(root, "adopt-work").unwrap();
    let original = std::fs::read_to_string(&manifest_path).unwrap();
    std::fs::write(
        &manifest_path,
        original
            .replace(r#""path": "source/outline.md""#, r#""path": "artifacts/outline.md""#)
            .replace("works/adopt-work/source/outline.md", "works/adopt-work/artifacts/outline.md"),
    )
    .unwrap();
    let (status, _) = request(
        &app,
        "POST",
        "/api/v1/works/adopt-work/artifacts/outline/revisions/r2/adopt",
        adopt_body("r1"),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    // 三次失败 = 三个 episode 行（全部 failed）；按 action-failed 事件码归类
    let store = CreativeEpisodeStore::new(&root.join(".inkos").join("harness.sqlite")).unwrap();
    let listed = store
        .list_episodes(&ListEpisodesOptions {
            work_id: Some("adopt-work".into()),
            profile_id: None,
            status: None,
            limit: Some(50),
        })
        .unwrap();
    assert_eq!(listed.len(), 3, "每次失败产生独立 episode 行");
    assert!(listed.iter().all(|episode| episode.status == inkos_engine::harness::EpisodeStatus::Failed));
    let mut codes = Vec::new();
    for episode in &listed {
        let events = store.list_events(&episode.id).unwrap();
        assert!(events.iter().any(|event| event.event_type == "episode-failed"));
        if let Some(failed) = events.iter().find(|event| event.event_type == "action-failed") {
            codes.push(failed.payload["code"].as_str().unwrap_or("(no code)").to_string());
        }
    }
    assert!(codes.iter().any(|code| code == "ARTIFACT_REVISION_CONFLICT"), "{codes:?}");
    assert!(codes.iter().any(|code| code == "ARTIFACT_DERIVED"), "{codes:?}");
    // 清单未被触碰（r1 仍为 current）
    let manifest = parse_work_manifest_json(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.artifacts[0].current_revision_id.as_deref(), Some("r1"));
    store.close();
}

#[tokio::test]
async fn adopt_fail_closed_on_domain_projection_schema() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // short-fiction profile + authority 文档路径 → 884 号 fail-closed 边界。
    // authority 路径 schema=short-manuscript：种子须为合法 JSON 才能越过
    // ARTIFACT_INVALID parse 门（TS 同序：先 parse 后 schema 校验），抵达
    // fail-closed 分支。
    seed_adopt_work(root, "short-work", "short-fiction", "source/final/short-story.json");
    let candidate = r#"{"chapters":[{"number":1,"title":"雨夜","content":"来客叩门；镜中人影。"}]}"#;
    let snapshot = root
        .join("works")
        .join("short-work")
        .join("revisions")
        .join("outline")
        .join("r2.md");
    std::fs::write(&snapshot, candidate).unwrap();
    let manifest_path = work_manifest_path(root, "short-work").unwrap();
    let original = std::fs::read_to_string(&manifest_path).unwrap();
    let patched = original
        .replace(&format!("\"checksum\": \"sha256:{}\"", sha256_hex(REV2_TEXT)), &format!("\"checksum\": \"sha256:{}\"", sha256_hex(candidate)))
        .replace(&format!("\"byteLength\": {}", REV2_TEXT.len()), &format!("\"byteLength\": {}", candidate.len()));
    std::fs::write(&manifest_path, patched).unwrap();

    let app = assemble(root);
    let (status, body) = request(
        &app,
        "POST",
        "/api/v1/works/short-work/artifacts/outline/revisions/r2/adopt",
        adopt_body("r1"),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "INTERNAL_ERROR");

    // action-failed 事件携带显式迁移边界码（而非静默裸写）
    let store = CreativeEpisodeStore::new(&root.join(".inkos").join("harness.sqlite")).unwrap();
    let listed = store
        .list_episodes(&ListEpisodesOptions {
            work_id: Some("short-work".into()),
            profile_id: None,
            status: None,
            limit: Some(50),
        })
        .unwrap();
    let events = store.list_events(&listed[0].id).unwrap();
    let failed = events.iter().find(|event| event.event_type == "action-failed").unwrap();
    assert_eq!(
        failed.payload["code"],
        "ARTIFACT_DOMAIN_PROJECTION_UNAVAILABLE",
        "L2 域投影分支 fail-closed（884 号迁移边界）"
    );
    store.close();

    // 清单未被触碰（对比：静默裸写会翻转 current 而不产出投影——腐化态）
    let manifest = parse_work_manifest_json(
        &std::fs::read_to_string(work_manifest_path(root, "short-work").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.artifacts[0].current_revision_id.as_deref(), Some("r1"));
}

// ── episodes 端点 ────────────────────────────────────────────────────

fn seed_episode(root: &Path, episode_id: &str, work_id: &str) {
    let store = CreativeEpisodeStore::new(&root.join(".inkos").join("harness.sqlite")).unwrap();
    let episode = serde_json::from_str::<inkos_engine::harness::CreativeEpisode>(&format!(
        r#"{{"version":2,"id":"{episode_id}","workId":"{work_id}","profileId":"workspace-default","status":"completed","startedAt":"2026-11-03T00:00:00.000Z","completedAt":"2026-11-03T00:01:00.000Z"}}"#
    ))
    .unwrap();
    store.create(&episode).unwrap();
    store
        .append(
            episode_id,
            "stage.completed",
            Some(work_id),
            Some("workspace"),
            None,
            &serde_json::json!({ "stage": "outline" }),
            "2026-11-03T00:00:30.000Z",
        )
        .unwrap();
    store.close();
}

#[tokio::test]
async fn episodes_endpoints_filters_and_error_face() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_episode(root, "ep-a", "work-a");
    seed_episode(root, "ep-b", "work-b");
    let app = assemble(root);

    let (status, body) = request(&app, "GET", "/api/v1/episodes", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["episodes"].as_array().unwrap().len(), 2);

    let (status, body) = request(&app, "GET", "/api/v1/episodes?workId=work-a", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed = body["episodes"].as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], "ep-a");

    let (status, body) = request(&app, "GET", "/api/v1/episodes?status=running", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["episodes"].as_array().unwrap().len(), 0);

    let (status, body) = request(&app, "GET", "/api/v1/episodes?limit=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["episodes"].as_array().unwrap().len(), 1);

    // 详情：episode + events
    let (status, body) = request(&app, "GET", "/api/v1/episodes/ep-a", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["episode"]["id"], "ep-a");
    assert_eq!(body["events"].as_array().unwrap().len(), 1);
    assert_eq!(body["events"][0]["type"], "stage.completed");

    // 未知 episode → 500 INTERNAL_ERROR 固定文案（TS 无局部 catch，勿"修复"为 404）
    let (status, body) = request(&app, "GET", "/api/v1/episodes/ghost", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "INTERNAL_ERROR");
    assert_eq!(body["error"]["message"], "Unexpected server error.");
}

// ── source_sync 确定性 ───────────────────────────────────────────────

#[test]
fn artifact_id_and_content_type_derivation_matches_ts() {
    use inkos_engine::harness::source_sync::{artifact_id_for, content_type_for};
    // TS artifactIdFor：可读段清洗（CJK 保留）+ hash10
    let id = artifact_id_for("source/outline.md");
    assert!(id.starts_with("outline-"), "{id}");
    assert_eq!(id.len(), "outline-".len() + 10);
    let cjk = artifact_id_for("source/镜花水月.md");
    assert!(cjk.starts_with("镜花水月-"), "CJK 保留：{cjk}");
    let messy = artifact_id_for("source/My Chapter (final v2)!.md");
    assert!(messy.starts_with("My-Chapter-final-v2-"), "清洗：{messy}");
    assert_eq!(content_type_for("a.md"), "text/markdown");
    assert_eq!(content_type_for("a.json"), "application/json");
    assert_eq!(content_type_for("a.png"), "image/png");
    assert_eq!(content_type_for("a.unknown"), "application/octet-stream");
}

#[tokio::test]
async fn source_sync_accept_new_file_supersedes_and_removes() {
    use inkos_engine::harness::source_sync::{sync_work_source_artifacts, SyncWorkSourceInput};
    use inkos_engine::utils::atomic_file_set::{AtomicFileWrite, FileContent};

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_adopt_work(root, "adopt-work", "workspace-default", "source/outline.md");
    let source_dir = root.join("works").join("adopt-work").join("source");

    // ① 新文件 + 既有文件更新，一次 accept 提交
    let new_text = "# 新章节\n\n内容。\n";
    let updated_outline = "# 大纲 v3\n\n第三版。\n";
    let output = sync_work_source_artifacts(SyncWorkSourceInput {
        project_root: root,
        work_id: "adopt-work",
        accept: true,
        accept_paths: None,
        episode_id: Some("ep-x"),
        updated_at: Some("2026-11-04T00:00:00.000Z".into()),
        writes: vec![
            AtomicFileWrite {
                relative_path: "works/adopt-work/source/outline.md".into(),
                content: FileContent::Text(updated_outline.into()),
            },
            AtomicFileWrite {
                relative_path: "works/adopt-work/source/chapters/0001.md".into(),
                content: FileContent::Text(new_text.into()),
            },
        ],
        title: None,
        status: None,
    })
    .await
    .unwrap();
    let artifact = output
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.id == "outline")
        .unwrap();
    let current_checksum = format!("sha256:{}", sha256_hex(updated_outline));
    assert_eq!(
        artifact
            .revisions
            .iter()
            .find(|revision| Some(&revision.id) == artifact.current_revision_id.as_ref())
            .unwrap()
            .checksum,
        current_checksum,
        "outline 翻转到新 checksum 修订"
    );
    let chapter = output
        .manifest
        .artifacts
        .iter()
        .find(|artifact| {
            artifact.revisions.iter().any(|revision| revision.path == "source/chapters/0001.md")
        })
        .unwrap();
    assert_eq!(chapter.kind, "chapter", "chapters/<n>.md → chapter kind");
    assert!(chapter.current_revision_id.is_some(), "accept 全量语义 = 新文件直接 current");
    // 源文件落盘
    assert_eq!(
        std::fs::read_to_string(source_dir.join("chapters").join("0001.md")).unwrap(),
        new_text
    );

    // ② 淘汰检测：删掉 0001.md 再同步 → 该工件 current 撤销 + removedAt/Path
    std::fs::remove_file(source_dir.join("chapters").join("0001.md")).unwrap();
    let output = sync_work_source_artifacts(SyncWorkSourceInput {
        project_root: root,
        work_id: "adopt-work",
        accept: true,
        accept_paths: None,
        episode_id: None,
        updated_at: Some("2026-11-05T00:00:00.000Z".into()),
        writes: vec![],
        title: None,
        status: None,
    })
    .await
    .unwrap();
    let chapter = output
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.metadata.get("removedPath").is_some())
        .expect("淘汰工件带 removedPath");
    assert_eq!(chapter.metadata["removedPath"], "source/chapters/0001.md");
    assert_eq!(chapter.metadata["removedAt"], "2026-11-05T00:00:00.000Z");
    assert!(chapter.current_revision_id.is_none());
    assert!(chapter
        .revisions
        .iter()
        .all(|revision| revision.status != inkos_engine::harness::ArtifactRevisionStatus::Current));
}

// ── skills documents ─────────────────────────────────────────────────

#[tokio::test]
async fn skill_documents_endpoint_lists_sorted_with_error_face() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".agents").join("skills").join("doc-skill");
    std::fs::create_dir_all(skill_dir.join("docs")).unwrap();
    let skill_markdown = "---\nname: doc-skill\ndescription: 文档编辑器种子技能\n---\n\n# doc-skill\n";
    std::fs::write(skill_dir.join("SKILL.md"), skill_markdown).unwrap();
    std::fs::write(skill_dir.join("docs").join("notes.md"), "# 笔记\n").unwrap();
    std::fs::write(skill_dir.join("docs").join("appendix.txt"), "附录\n").unwrap();
    // 非文档扩展名 + 符号链接不入列
    std::fs::write(skill_dir.join("data.json"), "{}").unwrap();
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(skill_dir.join("SKILL.md"), skill_dir.join("linked.md"));
    }
    let app = assemble(root);

    let (status, body) = request(&app, "GET", "/api/v1/skills/doc-skill/documents", None).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["id"], "doc-skill");
    let documents = body["documents"].as_array().unwrap();
    let paths: Vec<&str> = documents.iter().map(|doc| doc["path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        vec!["SKILL.md", "docs/appendix.txt", "docs/notes.md"],
        "按 path 排序（884 双端对齐）；data.json/符号链接不入列：{paths:?}"
    );
    assert_eq!(documents[0]["content"], skill_markdown);

    // 未知技能 → 404 typed
    let (status, body) = request(&app, "GET", "/api/v1/skills/ghost/documents", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "SKILL_NOT_FOUND");
    assert_eq!(body["error"]["message"], "Skill documents unavailable");
}
