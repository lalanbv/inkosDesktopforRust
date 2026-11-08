//! 927 号：origin 策略双端判定对照矩阵（Rust 侧）——include_str 单真源
//! 表（packages/core/src/__tests__/fixtures/cover-origin-cases.json，
//! TS 侧 cover-origin-matrix.test.ts 同表同断言），防双端判定漂移。
//! 表内用例仅收零网络确定性形态；DNS 解析面由各端单测注入 stub 覆盖。

use inkos_engine::llm::cover_origin::{
    assert_cover_download_url_allowed, resolve_cover_download_origin_policy,
};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct MatrixCase {
    url: String,
    policy: String,
    #[serde(default)]
    verdict: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default, rename = "extraOrigins")]
    extra_origins: Vec<String>,
}

#[derive(Deserialize)]
struct MatrixFile {
    provider: String,
    cases: Vec<MatrixCase>,
}

fn policy_for(policy: &str, extra_origins: &[String]) -> inkos_engine::llm::cover_origin::CoverDownloadOriginPolicy {
    let mut env = BTreeMap::new();
    match policy {
        "strict" => env.insert("INKOS_COVER_DOWNLOAD_ORIGIN_POLICY".to_string(), "strict".to_string()),
        "invalid" => env.insert("INKOS_COVER_DOWNLOAD_ORIGIN_POLICY".to_string(), "banana".to_string()),
        _ => None,
    };
    if !extra_origins.is_empty() {
        env.insert("INKOS_COVER_DOWNLOAD_EXTRA_ORIGINS".to_string(), extra_origins.join(","));
    }
    resolve_cover_download_origin_policy(&env)
}

#[tokio::test]
async fn rust_policy_matches_shared_matrix_table() {
    let raw = include_str!("../../packages/core/src/__tests__/fixtures/cover-origin-cases.json");
    let matrix: MatrixFile = serde_json::from_str(raw).expect("对照表解析失败");
    let mut mismatches: Vec<String> = Vec::new();
    for (index, case) in matrix.cases.iter().enumerate() {
        let policy = policy_for(&case.policy, &case.extra_origins);
        let got = match assert_cover_download_url_allowed(&case.url, &matrix.provider, &policy).await {
            Ok(()) => "allow".to_string(),
            Err(message) => {
                let reason = message
                    .split('(')
                    .nth(1)
                    .and_then(|rest| rest.split(')').next())
                    .unwrap_or("?")
                    .to_string();
                format!("deny:{reason}")
            }
        };
        let expected = match case.verdict.as_str() {
            "allow" => "allow".to_string(),
            _ => format!("deny:{}", case.reason.as_deref().unwrap_or("?")),
        };
        if expected != got {
            mismatches.push(format!(
                "#{index} {} [{}] 期望 {expected} 实得 {got}",
                case.url, case.policy
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "双端判定漂移 {} 条：\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
