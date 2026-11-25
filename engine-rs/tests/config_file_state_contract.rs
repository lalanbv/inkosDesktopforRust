//! 配置文件状态契约（inkos.json 缺失 / 损坏 / 根不是对象）——产品引擎侧。
//!
//! 契约（两个引擎同码同文；写入面 × 文件状态的「状态码 + 响应体」逐格取自
//! 两腿共用的对照表 `packages/studio/src/__tests__/golden/config-write-parity.json`，
//! Node 腿 `packages/studio/src/__tests__/config-file-state.test.ts` 读同一文件——
//! 任何一腿漂移都会在自己这边变红）：
//! - **文件缺失**：读面降级为「未配置」（200）；服务配置写入即新建。
//! - **文件损坏**（读不出 / 无法解析 / 根不是对象）：读面同样降级（200）；
//!   **写入一律拒绝**，文件逐字节不动。
//! - **诊断面** `GET /api/v1/doctor` 的 `inkosJson` = 文件存在且可解析为对象。
//! - **落盘失败**必须报错，且不继续动 secrets.json。
//!
//! 取证（修复前，真实二进制）：损坏的 inkos.json + `PUT /services/config`
//! → 200 并写出 43 字节的新文件，项目其余配置全部丢失；同一请求 Node 腿
//! 是 500 且文件原样保留。此前四个配置写入点用
//! `load_raw_config(…).unwrap_or(json!({}))`，把损坏文件当成了空配置。
//! `DELETE /services/:service` 则用 `let _ =` 吞掉落盘失败照样回 200，还接着把
//! 该服务的密钥删掉。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::http::StatusCode;
use serde_json::{json, Value};
use tower::ServiceExt;

const TABLE: &str =
    include_str!("../../packages/studio/src/__tests__/golden/config-write-parity.json");

/// 测试凭据一律 env 注入 + 非凭据回退。
fn test_key() -> String {
    std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".into())
}

/// 独立项目根；随值析构清理临时目录。
struct Project(tempfile::TempDir);

impl Project {
    fn new(config: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        if let Some(config) = config {
            std::fs::write(dir.path().join("inkos.json"), config).unwrap();
        }
        Self(dir)
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
    /// 配置文件当前内容；不存在时回 `None`（「仍然不存在」也是一种不动）。
    fn config_text(&self) -> Option<String> {
        std::fs::read_to_string(self.0.path().join("inkos.json")).ok()
    }
}

/// 经完整路由跑（与真实挂载一致），避免直接调 handler 的提取器差异。
fn app(root: &Path) -> axum::Router {
    use inkos_engine::llm::agent_router::{AgentRouter, LlmEndpointConfig};
    use inkos_engine::server::audit_route::AuditRuntime;
    use inkos_engine::server::books_routes::BooksRuntime;
    use inkos_engine::server::write_next_route::{WriteNextRunner, WriteNextRuntime};
    let hub = Arc::new(inkos_engine::server::sse::BroadcastHub::new());
    let state = Arc::new(inkos_engine::state::manager::StateManager::new(
        root.to_path_buf(),
    ));
    let router = Arc::new(AgentRouter::new(
        LlmEndpointConfig {
            context_window_tokens: 128_000,
            base_url: "http://127.0.0.1:9".into(),
            api_key: "k".into(),
            model: "m".into(),
            max_tokens: 4096,
            extra_headers: std::collections::HashMap::new(),
        },
        std::collections::HashMap::new(),
    ));
    let runner: WriteNextRunner = Arc::new(
        |_state: Arc<inkos_engine::state::manager::StateManager>,
         _book_id: String,
         _word_count: Option<u32>,
         _temperature: Option<f64>,
         _abort: inkos_engine::interaction::agent_loop::AbortHandle,
         _context: Option<String>| {
            Box::pin(async {
                Err::<inkos_engine::pipeline::write_next::ChapterPipelineResult, String>(
                    "config state contract: write face not exercised".to_string(),
                )
            })
        },
    );
    let write_next = WriteNextRuntime {
        hub: hub.clone(),
        state: state.clone(),
        runner,
        project_root: root.to_path_buf(),
    };
    let audit = AuditRuntime {
        hub: hub.clone(),
        state: state.clone(),
        router: router.clone(),
        builtin_genres_dir: root.join("assets").join("genres"),
    };
    let books = BooksRuntime {
        hub,
        state,
        router,
        builtin_genres_dir: root.join("assets").join("genres"),
        revision_gate: Default::default(),
    };
    inkos_engine::server::router_books(
        inkos_engine::server::AppState {
            version: "test".to_string(),
        },
        write_next.hub.clone(),
        write_next,
        audit,
        books,
    )
}

async fn call(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = axum::http::Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(axum::body::Body::from(body.unwrap_or_default().to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

/// 可导入的项目 `.env`。
fn write_importable_env(root: &Path) {
    std::fs::write(
        root.join(".env"),
        format!(
            "INKOS_LLM_API_KEY={}\nINKOS_LLM_BASE_URL=http://127.0.0.1:9/v1\n",
            test_key()
        ),
    )
    .unwrap();
}

fn table() -> Value {
    serde_json::from_str(TABLE).unwrap()
}

/// 打一格并断言：状态码、响应体逐字、inkos.json 逐字节不动。
async fn assert_cell(
    label: &str,
    project: &Project,
    method: &str,
    path: &str,
    body: Option<&str>,
    shape: &Value,
) {
    let before = project.config_text();
    let (status, parsed) = call(app(project.path()), method, path, body).await;
    assert_eq!(
        status,
        StatusCode::from_u16(shape["status"].as_u64().unwrap() as u16).unwrap(),
        "{label}"
    );
    assert_eq!(parsed, shape["response"], "{label}");
    assert_eq!(project.config_text(), before, "{label}：文件必须逐字节不动");
}

/// 已配置项目的密钥（对照表 `bodyCases`：需要模型的面，密钥只走 secrets.json、
/// 不带 `.env`——`.env` 会被识别成 env 配置形态）。
fn seed_configured_secrets(root: &Path) {
    inkos_engine::llm::secrets::save_secrets(
        root,
        &inkos_engine::llm::secrets::SecretsFile {
            services: [(
                "custom:Mock".to_string(),
                inkos_engine::llm::secrets::ServiceSecret {
                    api_key: test_key(),
                },
            )]
            .into_iter()
            .collect(),
        },
    )
    .unwrap();
}

#[tokio::test]
async fn parity_matrix_refusals_match_the_shared_table() {
    let table = table();
    let shapes = table["shapes"].as_object().unwrap();
    let states: Vec<(&str, Option<&str>)> = table["states"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(state, content)| (state.as_str(), content.as_str()))
        .collect();
    assert_eq!(states.len(), 7);
    let matrix = table["matrix"].as_array().unwrap();
    assert_eq!(matrix.len(), 13);
    let state_names: BTreeSet<&str> = states.iter().map(|(name, _)| *name).collect();
    let mut checked = 0usize;
    for row in matrix {
        let method = row["method"].as_str().unwrap();
        let path = row["path"].as_str().unwrap();
        let body = row["body"].as_str();
        let needs_env = row["env"].as_bool().unwrap_or(false);
        // 每行必须列全全部状态：状态名写错会让那一格被静默跳过。
        let listed: BTreeSet<&str> = row["expect"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(listed, state_names, "{method} {path}");
        for (state, content) in &states {
            // null = 该格不在本表断言（成功路径）。
            let Some(shape) = row["expect"][state].as_str() else {
                continue;
            };
            let project = Project::new(*content);
            if needs_env {
                write_importable_env(project.path());
            }
            assert_cell(
                &format!("{method} {path} [{state}]"),
                &project,
                method,
                path,
                body,
                &shapes[shape],
            )
            .await;
            checked += 1;
        }
    }
    // 4 × 6（服务配置族：缺失即新建，不在本表）+ 9 × 7（`/project` 族）：表被
    // 删行 / 删格时立刻可见。
    assert_eq!(checked, 87);
}

#[tokio::test]
async fn parity_special_cases_match_the_shared_table() {
    let table = table();
    let shapes = table["shapes"].as_object().unwrap();
    let cases = table["cases"].as_array().unwrap();
    assert!(cases.len() >= 7, "专项用例被删");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let project = Project::new(case["config"].as_str());
        assert_cell(
            name,
            &project,
            case["method"].as_str().unwrap(),
            case["path"].as_str().unwrap(),
            case["body"].as_str(),
            &shapes[case["shape"].as_str().unwrap()],
        )
        .await;
    }
}

/// 请求体不是 JSON × 需要模型的面（已配置项目，守卫放行后才到处理器的请求体
/// 解析）。这些处理器此前手写平铺 `"Unexpected token"`，与 Node 的 onError 信封
/// 不同形。
#[tokio::test]
async fn parity_malformed_bodies_on_model_faces_match_the_shared_table() {
    let table = table();
    let shapes = table["shapes"].as_object().unwrap();
    let group = &table["bodyCases"];
    assert!(
        group["secrets"].as_bool().unwrap(),
        "这组用例依赖 secrets 夹具"
    );
    let requests = group["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 6, "畸形请求体用例被删");
    for request in requests {
        let project = Project::new(Some(group["config"].as_str().unwrap()));
        seed_configured_secrets(project.path());
        assert_cell(
            &format!(
                "{} {}",
                request["method"].as_str().unwrap(),
                request["path"].as_str().unwrap()
            ),
            &project,
            request["method"].as_str().unwrap(),
            request["path"].as_str().unwrap(),
            request["body"].as_str(),
            &shapes[group["shape"].as_str().unwrap()],
        )
        .await;
    }
}

#[tokio::test]
async fn missing_config_still_creates_on_write() {
    let project = Project::new(None);
    write_importable_env(project.path());

    let (status, parsed) = call(
        app(project.path()),
        "PUT",
        "/api/v1/services/config",
        Some(r#"{"defaultModel":"m1"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{parsed}");
    let created: Value = serde_json::from_str(&project.config_text().unwrap()).unwrap();
    assert_eq!(created["llm"]["defaultModel"], "m1", "写入即新建");

    // 其余三个写入点在文件存在且合法时照常成功（拒绝只针对「读不出来」）。
    for (method, uri, body) in [
        ("DELETE", "/api/v1/services/custom:X", ""),
        ("POST", "/api/v1/services/config/import-env", ""),
        ("PUT", "/api/v1/cover/config", r#"{"service":"kkaiapi"}"#),
    ] {
        let (status, parsed) = call(app(project.path()), method, uri, Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{method} {uri}: {parsed}");
    }
}

#[tokio::test]
async fn reads_degrade_on_missing_and_corrupt_config() {
    for content in [None, Some("{ not json"), Some("[1]")] {
        let project = Project::new(content);
        let (_, services) = call(app(project.path()), "GET", "/api/v1/services/config", None).await;
        assert_eq!(services["services"], json!([]), "inkos.json = {content:?}");
        let (status, cover) = call(app(project.path()), "GET", "/api/v1/cover/config", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cover["configured"], false);
        let (status, budget) = call(
            app(project.path()),
            "GET",
            "/api/v1/usage/budget-status",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(budget["statuses"], json!([]));
        assert_eq!(project.config_text(), content.map(str::to_string));
    }
}

#[tokio::test]
async fn doctor_reports_unhealthy_config() {
    for (content, healthy) in [
        (None, false),
        (Some("{ not json"), false),
        (Some("[]"), false),
        (Some(r#"{"name":"x"}"#), true),
    ] {
        let project = Project::new(content);
        let (status, parsed) = call(app(project.path()), "GET", "/api/v1/doctor", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(parsed["inkosJson"], healthy, "inkos.json = {content:?}");
    }
}

/// 目录只读 → 读取照常、落盘失败。删除服务必须报错并停在这里：inkos.json 与
/// secrets.json 都不动。此前 `let _ = save_raw_config(…)` 吞掉写失败照样回 200，
/// 还接着把该服务的密钥删掉——配置里服务还在、密钥没了。
#[cfg(unix)]
#[tokio::test]
async fn failed_config_write_reports_the_failure_and_keeps_secrets() {
    use inkos_engine::llm::secrets::{load_secrets, save_secrets, SecretsFile, ServiceSecret};
    use std::os::unix::fs::PermissionsExt;

    let original =
        r#"{"name":"x","llm":{"services":[{"service":"custom","name":"X"}],"service":"custom:X"}}"#;
    let project = Project::new(Some(original));
    let service = "custom:X".to_string();
    let key = test_key();
    save_secrets(
        project.path(),
        &SecretsFile {
            services: [(
                service.clone(),
                ServiceSecret {
                    api_key: key.clone(),
                },
            )]
            .into_iter()
            .collect(),
        },
    )
    .unwrap();

    // 只读目录无论测试是否 panic 都要还原，否则临时目录清不掉。
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let _restore = Restore(project.path().to_path_buf());
    std::fs::set_permissions(project.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    // 夹具自检：本机必须真的强制只读权限，否则下面验证不到「落盘失败」。
    assert!(
        std::fs::File::create(project.path().join(".probe")).is_err(),
        "运行身份不受只读权限约束（root？），无法构造落盘失败"
    );

    // 读取照常成功——测的是「读得到、写不进」。
    let (status, _) = call(app(project.path()), "GET", "/api/v1/services/config", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, parsed) = call(
        app(project.path()),
        "DELETE",
        "/api/v1/services/custom:X",
        Some(""),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{parsed}");
    assert_eq!(parsed, json!({ "error": "failed to save inkos.json" }));
    assert_eq!(project.config_text().as_deref(), Some(original));
    let secrets = load_secrets(project.path()).unwrap();
    assert_eq!(
        secrets
            .services
            .get(&service)
            .map(|secret| secret.api_key.as_str()),
        Some(key.as_str()),
        "落盘失败后不得动密钥"
    );
}
