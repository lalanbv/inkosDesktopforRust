//! work 域 M2 写面测试（885 号）：profiles POST/PUT（创作配置编辑器保存面）+
//! skills documents PUT（编辑器写面）——`packages/studio/src/api/server.ts` 对应
//! 端点的 Rust 腿契约面。
//!
//! 错误面对齐（勿"修复"）：TS 两个写面端点均无局部 catch——zod parse 失败/body
//! 异常→500 INTERNAL_ERROR 固定文案；typed 400/409 仅显式 ApiError 面
//!（PROFILE_EXISTS / PROFILE_ID_MISMATCH / INVALID_SKILL_MANIFEST /
//! SKILL_ID_MISMATCH / INVALID_SKILL_PATH）。
//!
//! 与 TS 侧 `packages/core/src/__tests__/work-store-cross-engine.test.ts` 共用
//! profile 金标准（884 号起双端恒等锁）。

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

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
                    "M2 契约测试不触写链".to_string(),
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
        inkos_engine::server::AppState { version: "work-m2".into() },
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
    let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, value)
}

fn custom_profile_body(id: &str, title: &str) -> String {
    json!({
        "version": 2,
        "id": id,
        "title": title,
        "description": "自定义创作配置",
        "capabilityIds": ["workspace"],
        "requiredSkillIds": [],
        "recommendedSkillIds": [],
        "artifactKinds": [],
        "confirmation": {
            "inferredMutation": "execute",
            "explicitRecoverableMutation": "execute",
            "destructiveMutation": "confirm"
        },
        "artifactSchemas": {},
        "qualityCriteria": [],
        "production": {}
    })
    .to_string()
}

#[tokio::test]
async fn post_profile_creates_custom_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let app = assemble(root);

    let (status, body) = request(&app, "POST", "/api/v1/profiles", Some(custom_profile_body("zz-m2", "自定义二"))).await;
    assert_eq!(status, StatusCode::CREATED, "{}", body);
    assert_eq!(body["profile"]["id"], "zz-m2");
    assert_eq!(body["profile"]["title"], "自定义二");

    // 落盘文件在位（.inkos/profiles/<id>.json）
    let file = root.join(".inkos").join("profiles").join("zz-m2.json");
    assert!(file.exists(), "自定义 profile 落盘");
    let raw = std::fs::read_to_string(&file).unwrap();
    let parsed: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed["id"], "zz-m2");
    assert!(!raw.ends_with('\n'), "TS JSON.stringify 无尾换行（work.json 才有）");

    // GET /profiles 归并入列（10 = 内置 9 + 自定义 1）
    let (status, body) = request(&app, "GET", "/api/v1/profiles", None).await;
    assert_eq!(status, StatusCode::OK);
    let profiles = body["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 10);
    assert!(profiles.iter().any(|profile| profile["id"] == "zz-m2"));

    // 重复 POST（自定义重名）→ 409 PROFILE_EXISTS
    let (status, body) = request(&app, "POST", "/api/v1/profiles", Some(custom_profile_body("zz-m2", "重复"))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "PROFILE_EXISTS");
    assert_eq!(body["error"]["message"], "Choose a new profile ID");

    // 内置 id 重名 → 同 409（registry 全集判断，TS 同）
    let (status, body) = request(&app, "POST", "/api/v1/profiles", Some(custom_profile_body("short-fiction", "冒名"))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "PROFILE_EXISTS");

    // 非法 body（version≠2）→ 500 INTERNAL_ERROR 固定文案（zod parse 抛普通错）。
    // json!().to_string() 无空格——replace 须按紧凑形态匹配（885 号测试自坑修正）。
    let bad = custom_profile_body("bad-m2", "坏").replace("\"version\":2", "\"version\":3");
    let (status, body) = request(&app, "POST", "/api/v1/profiles", Some(bad)).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "INTERNAL_ERROR");
    assert_eq!(body["error"]["message"], "Unexpected server error.");

    // 缺 body → 500 固定文案
    let (status, body) = request(&app, "POST", "/api/v1/profiles", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["message"], "Unexpected server error.");
}

#[tokio::test]
async fn put_profile_overrides_builtin_and_rejects_id_change() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let app = assemble(root);

    // PUT 内置 short-fiction（标题覆盖）→ 200 + 落盘覆盖文件
    let overridden = GOLDEN_SHORT_FICTION_PROFILE_JSON
        .replace("\"title\": \"Short fiction\"", "\"title\": \"短篇（编辑器覆盖）\"");
    let (status, body) = request(&app, "PUT", "/api/v1/profiles/short-fiction", Some(overridden)).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["profile"]["title"], "短篇（编辑器覆盖）");
    assert!(root.join(".inkos").join("profiles").join("short-fiction.json").exists());
    let (_status, body) = request(&app, "GET", "/api/v1/profiles", None).await;
    let overridden_served = body["profiles"].as_array().unwrap().iter()
        .find(|profile| profile["id"] == "short-fiction").unwrap().clone();
    assert_eq!(overridden_served["title"], "短篇（编辑器覆盖）");
    assert_eq!(overridden_served["artifactSchemas"]["source/final/short-story.json"], "short-manuscript");

    // id 不可变：body.id ≠ 路径 id → 400 PROFILE_ID_MISMATCH
    let (status, body) = request(
        &app,
        "PUT",
        "/api/v1/profiles/other-id",
        Some(custom_profile_body("zz-m2", "改名")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "PROFILE_ID_MISMATCH");
    assert_eq!(body["error"]["message"], "Profile identity cannot change");

    // PUT 新 id（无同名内置/自定义）→ 200（upsert 语义，TS 同——PUT 无存在性检查）
    let (status, body) = request(&app, "PUT", "/api/v1/profiles/zz-m2", Some(custom_profile_body("zz-m2", "新落"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["profile"]["title"], "新落");
    assert!(root.join(".inkos").join("profiles").join("zz-m2.json").exists());
}

fn seed_project_skill(root: &Path) {
    let skill_dir = root.join(".agents").join("skills").join("edit-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: edit-skill\ndescription: 编辑器写面种子技能\n---\n\n# edit-skill\n",
    )
    .unwrap();
}

#[tokio::test]
async fn put_skill_documents_writes_and_validates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_project_skill(root);
    let app = assemble(root);

    // 快乐路径：SKILL.md + 新文档 → 200 saved 列表 + GET 反映
    let payload = json!({
        "documents": [
            { "path": "SKILL.md", "content": "---\nname: edit-skill\ndescription: 编辑器写面种子技能\n---\n\n# edit-skill 更新\n" },
            { "path": "docs/extra.md", "content": "# 追加文档\n" },
        ],
    });
    let (status, body) = request(
        &app,
        "PUT",
        "/api/v1/skills/edit-skill/documents",
        Some(payload.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    assert_eq!(body["id"], "edit-skill");
    let saved = body["saved"].as_array().unwrap();
    assert_eq!(saved.len(), 2);
    let (status, body) = request(&app, "GET", "/api/v1/skills/edit-skill/documents", None).await;
    assert_eq!(status, StatusCode::OK);
    let documents = body["documents"].as_array().unwrap();
    assert!(documents.iter().any(|doc| doc["path"] == "docs/extra.md" && doc["content"] == "# 追加文档\n"));
    assert!(documents.iter().any(|doc| doc["path"] == "SKILL.md" && doc["content"].as_str().unwrap().contains("更新")));

    // SKILL.md 缺失（根）双面：仅 docs 文件 → 共享归一化先拒 INVALID_SKILL_IMPORT
    //（TS 同序）；嵌套 sub/SKILL.md 过归一化但不在根 → INVALID_SKILL_MANIFEST。
    let (status, body) = request(
        &app,
        "PUT",
        "/api/v1/skills/edit-skill/documents",
        Some(json!({ "documents": [{ "path": "docs/only.md", "content": "x" }] }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_SKILL_IMPORT");
    let (status, body) = request(
        &app,
        "PUT",
        "/api/v1/skills/edit-skill/documents",
        Some(json!({ "documents": [
            { "path": "sub/SKILL.md", "content": "---\nname: edit-skill\ndescription: x\n---\n" }
        ] }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_SKILL_MANIFEST");
    assert_eq!(body["error"]["message"], "SKILL.md must be at the folder root");

    // 身份不可变（SKILL.md name ≠ 路径 id）→ 400 SKILL_ID_MISMATCH
    let (status, body) = request(
        &app,
        "PUT",
        "/api/v1/skills/edit-skill/documents",
        Some(json!({ "documents": [{ "path": "SKILL.md", "content": "---\nname: other-name\ndescription: x\n---\n" }] }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "SKILL_ID_MISMATCH");
    assert_eq!(body["error"]["message"], "Skill identity cannot change");

    // 越界路径（.. 段）→ 共享归一化 400
    let (status, _) = request(
        &app,
        "PUT",
        "/api/v1/skills/edit-skill/documents",
        Some(json!({ "documents": [{ "path": "../escape.md", "content": "x" }, { "path": "SKILL.md", "content": "---\nname: edit-skill\ndescription: x\n---\n" }] }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 未知技能 id 形状非法（大写/空格）→ 400 INVALID_SKILL_ID
    let (status, body) = request(
        &app,
        "PUT",
        "/api/v1/skills/Bad%20Id/documents",
        Some(json!({ "documents": [] }).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_SKILL_ID");

    // documents 缺失 → 500 固定文案（TS TypeError 无局部 catch）
    let (status, body) = request(&app, "PUT", "/api/v1/skills/edit-skill/documents", Some(json!({}).to_string())).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["message"], "Unexpected server error.");
}

#[tokio::test]
async fn put_skill_documents_rejects_symlink_segment() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    seed_project_skill(root);
    // docs 目录替换为符号链接 → 逐段检查 400 INVALID_SKILL_PATH
    let skill_dir = root.join(".agents").join("skills").join("edit-skill");
    std::fs::create_dir_all(skill_dir.join("docs")).unwrap();
    std::fs::write(skill_dir.join("docs").join("placeholder.md"), "x").unwrap();
    std::fs::remove_dir_all(skill_dir.join("docs")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("elsewhere"), skill_dir.join("docs")).unwrap();

    let app = assemble(root);
    let payload = json!({
        "documents": [
            { "path": "SKILL.md", "content": "---\nname: edit-skill\ndescription: 编辑器写面种子技能\n---\n" },
            { "path": "docs/new.md", "content": "x" },
        ],
    });
    let (status, body) = request(
        &app,
        "PUT",
        "/api/v1/skills/edit-skill/documents",
        Some(payload.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "INVALID_SKILL_PATH");
    assert_eq!(body["error"]["message"], "Skill editor cannot follow symlinks");
}
