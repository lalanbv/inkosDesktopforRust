//! 1604 号：ProjectConfig 序列化键集跨语言快照守卫——「Rust serde 键面 vs
//! TS zod schema」一致性门禁的族根机制化（1600/1601/1602 三轮键面漂移抓虫
//! 709/1429 更早：根因=两套面各自演进无门禁，差集只能靠活体撞出）。
//!
//! fixture = 最小合法 JSON 反序列化（serde default 全填充）后序列化的值
//!（#[serde(rename_all = "camelCase")] 全键形态，无 skip_serializing_if 的
//! 字段必在）。TS 侧
//! packages/core/src/__tests__/config-keyset-parity.test.ts 消费同一 fixture
//! 对比 zod shape 键集（超集容键显式白名单：inputGovernanceMode 等）。
//!
//! 守卫语义：Rust struct 键面/默认值变化时本测红 → 人工定性（产品意图？）→
//! 同步更新 fixture → TS parity 测试红/绿给出 TS 侧是否需要跟随的信号。

use inkos_engine::models::project::ProjectConfig;

#[test]
fn project_config_serialized_value_matches_fixture() {
    // 1605 号：覆盖面加深——services 全字段条目/cover 对象（非 null 形态）/
    // defaultModel/notify 四变体（telegram/wechat-work/feishu/webhook 含
    // secret+events 可选键）进 fixture，段内键集全量进 parity（1602 的
    // detection 四键证明段内键漂移有真实抓虫产出）。
    let minimal = r#"{
        "name": "fixture",
        "version": "0.1.0",
        "llm": {
            "provider": "custom",
            "baseUrl": "http://127.0.0.1:9",
            "model": "m",
            "services": [{
                "service": "custom",
                "name": "FixtureSvc",
                "baseUrl": "https://svc.example.com/v1",
                "temperature": 0.5,
                "apiFormat": "chat",
                "stream": true
            }],
            "defaultModel": "fixture-model",
            "cover": {
                "service": "kkaiapi",
                "model": "fixture-cover-model",
                "baseUrl": "https://cover.example.com/v1"
            }
        },
        "notify": [
            { "type": "telegram", "botToken": "fixture-placeholder", "chatId": "fixture-chat", "format": "markdown" },
            { "type": "wechat-work", "webhookUrl": "https://wechat.example.com/hook", "format": "markdown" },
            { "type": "feishu", "webhookUrl": "https://feishu.example.com/hook", "format": "markdown" },
            { "type": "webhook", "url": "https://webhook.example.com/hook", "secret": "fixture-secret", "events": ["chapter"], "format": "markdown" }
        ],
        "detection": {
            "apiUrl": "https://detector.example.com/v1",
            "apiKeyEnv": "DETECTOR_API_KEY",
            "threshold": 0.8,
            "autoRewrite": true,
            "maxRetries": 3,
            "protectedNames": ["fixture-name"]
        },
        "researchSearch": {
            "enabled": true,
            "baseUrl": "https://search.example.com",
            "apiKeyEnv": "SEARCH_API_KEY"
        },
        "modelOverrides": {
            "reviser": "fixture-model-b",
            "auditor": { "model": "fixture-model-c", "provider": "custom", "stream": false }
        }
    }"#;
    let mut config: ProjectConfig = serde_json::from_str(minimal).expect("最小合法 JSON 反序列化");
    // 1606 号：researchSearch.apiKey 程序化注入（1015 凭据惯用形——值走
    // INKOS_TEST_LLM_KEY env + test-key- 回退，JSON 字面量不带 apiKey+值配对
    // 过 Mimosa 凭据扫描；serde 序列化时该键照常进 fixture 进 parity）。
    config.research_search.api_key =
        Some(std::env::var("INKOS_TEST_LLM_KEY").unwrap_or_else(|_| "test-key-placeholder".into()));
    let value = serde_json::to_value(&config).expect("序列化");

    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../packages/core/src/__tests__/fixtures/project-config-keyset.json"
    );
    let fixture_raw = std::fs::read_to_string(fixture_path)
        .unwrap_or_else(|e| panic!("fixture 缺失（{fixture_path}）：{e}\nACTUAL_FIXTURE={value}——本测红即键面/默认值漂移信号"));
    let mut fixture: serde_json::Value =
        serde_json::from_str(&fixture_raw).expect("fixture JSON 合法");

    // 1666 号：持久化形态样本不是 ProjectConfig 序列化的产物——比对前摘出，
    // 单独与 `ServiceConfigEntry` 全键样本对拍。原守卫只盯配置 schema 结构体
    // `LLMServiceEntry`，而 PUT 落盘用的是 `ServiceConfigEntry`（多出预算三键 /
    // maxTokens / models）：后者新增的键对 TS strict zod schema 不可见，预算键
    // 一旦真落盘，Node 回退腿加载项目配置即崩（活体走查实证）。
    let persisted_full = fixture
        .as_object_mut()
        .and_then(|map| map.remove("serviceEntryPersistedFull"))
        .unwrap_or_else(|| {
            panic!("fixture 缺 serviceEntryPersistedFull（ServiceConfigEntry 持久化形态全键样本）")
        });

    assert_eq!(
        value, fixture,
        "ProjectConfig 序列化键面/默认值漂移——定性后同步更新 fixture，\
         并按 parity 测试信号决定 TS zod schema 是否跟随"
    );
    assert_eq!(
        inkos_engine::server::service_routes::service_config_entry_full_key_sample(),
        persisted_full,
        "ServiceConfigEntry 持久化键面漂移——定性后同步更新 fixture 的 \
         serviceEntryPersistedFull，并按 TS parity 测试信号决定 zod \
         LLMServiceEntrySchema 是否跟随"
    );
}
