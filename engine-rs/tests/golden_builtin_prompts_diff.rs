//! R54/1662 号：builtin prompts 数据单源守卫——TS Node 回退腿消费
//! `packages/core/src/prompts/builtin-prompts.json`（dump_builtin_prompts
//! example 导出），本测试断言 JSON 与 Rust RAW_BUILTIN_PROMPTS 逐字段相等
//! （漂移即红——builtin 变更须 `cargo run --example dump_builtin_prompts`
//! 重新导出，防双端数据面漂移）。

use inkos_engine::prompts::{builtin_prompt_packs, builtin_prompts};

const DATA: &str = include_str!("../../packages/core/src/prompts/builtin-prompts.json");

#[derive(serde::Deserialize)]
struct DataFile {
    packs: Vec<PackEntry>,
    prompts: Vec<PromptEntry>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackEntry {
    id: String,
    title: String,
    description: String,
    prompts: Vec<String>,
    source: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptEntry {
    id: String,
    #[serde(rename = "packId")]
    pack_id: String,
    title: String,
    content: String,
}

#[test]
fn builtin_prompts_data_matches_rust_source() {
    let data: DataFile = serde_json::from_str(DATA).expect("数据文件可解析");
    let packs = builtin_prompt_packs();
    let prompts = builtin_prompts();
    assert_eq!(data.packs.len(), packs.len(), "pack 数漂移");
    assert_eq!(data.prompts.len(), prompts.len(), "prompt 数漂移");
    for (entry, pack) in data.packs.iter().zip(&packs) {
        assert_eq!(entry.id, pack.id, "pack id 漂移");
        assert_eq!(entry.title, pack.title, "pack title 漂移：{}", pack.id);
        assert_eq!(
            entry.description, pack.description,
            "pack description 漂移：{}",
            pack.id
        );
        assert_eq!(
            entry.prompts, pack.prompts,
            "pack prompts 漂移：{}",
            pack.id
        );
        assert_eq!(entry.source, "builtin");
    }
    for (entry, prompt) in data.prompts.iter().zip(prompts.iter()) {
        assert_eq!(entry.id, prompt.id, "prompt id 漂移");
        assert_eq!(
            entry.pack_id, prompt.pack_id,
            "prompt packId 漂移：{}",
            prompt.id
        );
        assert_eq!(
            entry.title, prompt.title,
            "prompt title 漂移：{}",
            prompt.id
        );
        assert_eq!(
            entry.content, prompt.content,
            "prompt content 漂移：{}",
            prompt.id
        );
    }
}
