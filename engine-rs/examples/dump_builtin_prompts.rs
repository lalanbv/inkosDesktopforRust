//! 临时（R54/1662 号）：导出 builtin prompts 数据 JSON——单源数据落盘
//! `packages/core/src/prompts/builtin-prompts.json`（TS Node 回退腿消费）；
//! 等值守卫 = tests/golden_builtin_prompts_diff.rs（Rust 侧锁数据漂移）。
//! 生成命令：`cargo run --example dump_builtin_prompts`（导出后本文件留仓，
//! 复跑可再生成对拍）。
fn main() {
    let packs = inkos_engine::prompts::builtin_prompt_packs();
    let prompts = inkos_engine::prompts::builtin_prompts();
    let packs_json: Vec<serde_json::Value> = packs
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "title": p.title, "description": p.description,
                "prompts": p.prompts, "source": "builtin",
            })
        })
        .collect();
    let prompts_json: Vec<serde_json::Value> = prompts
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id, "packId": p.pack_id, "title": p.title, "content": p.content,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"packs": packs_json, "prompts": prompts_json})
        )
        .unwrap()
    );
}
