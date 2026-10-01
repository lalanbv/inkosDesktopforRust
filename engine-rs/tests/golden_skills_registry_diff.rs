//! R44/566 号：skills 注册表 rank 决胜共享 golden 差分。
//!
//! 唯一事实源 = `packages/core/src/__tests__/golden/skills-registry-vectors.json`
//! （TS 侧 skills-registry.test.ts 以同一 golden 锁定 createSkillRegistry）。
//! 本测试逐 case 断言同 id 决胜=有效 rank 升序（显式 frontmatter rank 优先于
//! 装载层缺省表 project skills 100 到 builtin 600）、平秩后写胜、rank 缺省
//! 退化为纯后写胜（R44 前行为兼容面）。改向量必须双端同批。
use inkos_engine::skills::create_skill_registry;
use inkos_engine::skills::AgentSkill;
use inkos_engine::skills::SkillRegistry;
use inkos_engine::skills::SkillSource;
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../packages/core/src/__tests__/golden/skills-registry-vectors.json");

fn make_skill(id: &str, rank: Option<u32>) -> AgentSkill {
    AgentSkill {
        id: id.to_string(),
        name: id.to_string(),
        description: format!("d-{id}"),
        body: String::new(),
        source: SkillSource::Project,
        base_dir: None,
        disable_model_invocation: None,
        user_invocable: None,
        rank,
    }
}

#[test]
fn golden_skills_registry_rank_resolution() {
    let vectors: Value = serde_json::from_str(VECTORS).expect("vectors json");
    assert_eq!(vectors["version"].as_u64(), Some(1));
    let cases = vectors["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());

    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let skills: Vec<AgentSkill> = case["skills"]
            .as_array()
            .expect("skills")
            .iter()
            .map(|skill| {
                make_skill(
                    skill["id"].as_str().expect("id"),
                    skill["rank"].as_u64().map(|n| n as u32),
                )
            })
            .collect();
        let registry = create_skill_registry(skills);
        let listed = registry.list_skills();
        let ids: Vec<&str> = listed.iter().map(|skill| skill.id.as_str()).collect();
        let expected_ids: Vec<&str> = case["expected"]["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().expect("id"))
            .collect();
        assert_eq!(ids, expected_ids, "{name}: 幸存 id 序");

        let winner_ranks = case["expected"]["winnerRanks"].as_object().expect("winnerRanks");
        for skill in &listed {
            let expected_rank = &winner_ranks[&skill.id];
            let expected = expected_rank.as_u64().map(|n| n as u32);
            assert_eq!(skill.rank, expected, "{name}: {}/{} 幸存 rank", skill.id, name);
        }
    }
}
