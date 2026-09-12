//! R4 三库资产生态契约层首批（362 号，二轮 P1）——统一 shape + 内容指纹
//! 幂等合并 + 便携包导入导出 + 题材基底种子。
//!
//! TS 真源：`packages/core/src/utils/asset-library.ts`；golden 唯一事实源：
//! `packages/core/src/__tests__/golden/asset-library-vectors.json`（差分测试
//! `tests/golden_asset_library_diff.rs` 读同一文件）。
//!
//! 移植纪律：canonical 拼接/FNV 逐字对齐 TS；排序纯码元比较（禁 locale）；
//! export 字符串只锁语义（键序+条目序）不锁缩进字节。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ASSET_LIBRARY_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum AssetKind {
    GenreBase,
    ProgressionMode,
    WorldSample,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::GenreBase => "genre-base",
            AssetKind::ProgressionMode => "progression-mode",
            AssetKind::WorldSample => "world-sample",
        }
    }

    fn parse(value: &str) -> Option<AssetKind> {
        match value {
            "genre-base" => Some(AssetKind::GenreBase),
            "progression-mode" => Some(AssetKind::ProgressionMode),
            "world-sample" => Some(AssetKind::WorldSample),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LibraryAsset {
    pub id: String,
    pub kind: AssetKind,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub expectations: Vec<String>,
    #[serde(default)]
    pub taboos: Vec<String>,
    #[serde(default)]
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetValidateResult {
    pub asset: Option<LibraryAsset>,
    pub errors: Vec<String>,
}

fn is_snake_case_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn valid_string_list(value: Option<&Vec<String>>, max_items: usize, max_chars: usize) -> bool {
    match value {
        None => true,
        Some(items) => {
            items.len() <= max_items
                && items.iter().all(|item| !item.is_empty() && item.chars().count() <= max_chars)
        }
    }
}

/// 单条资产校验：非法返回字段级错误清单（`{field}: message`，不抛错、不截断）。
pub fn validate_library_asset(raw: &Value) -> AssetValidateResult {
    let mut errors: Vec<String> = Vec::new();
    let obj = match raw.as_object() {
        Some(obj) => obj,
        None => return AssetValidateResult { asset: None, errors: vec!["(root): expected object".to_string()] },
    };

    let id = obj.get("id").and_then(Value::as_str).unwrap_or_default();
    if !is_snake_case_slug(id) {
        errors.push("id: must be snake_case slug (a-z0-9_, 1-64 chars)".to_string());
    }
    let kind_raw = obj.get("kind").and_then(Value::as_str).unwrap_or_default();
    let kind = AssetKind::parse(kind_raw);
    if kind.is_none() {
        errors.push("kind: must be genre-base | progression-mode | world-sample".to_string());
    }
    let name = obj.get("name").and_then(Value::as_str).unwrap_or_default();
    if name.is_empty() || name.chars().count() > 80 {
        errors.push("name: required, 1-80 chars".to_string());
    }
    let body = obj.get("body").and_then(Value::as_str).unwrap_or_default();
    if body.is_empty() || body.chars().count() > 4000 {
        errors.push("body: required, 1-4000 chars".to_string());
    }
    let expectations = obj.get("expectations").and_then(Value::as_array);
    if let Some(items) = expectations {
        if !valid_string_list(
            Some(
                &items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            ),
            20,
            200,
        ) || items.len() > 20
        {
            errors.push("expectations: max 20 items, 1-200 chars each".to_string());
        }
    }
    let taboos = obj.get("taboos").and_then(Value::as_array);
    if let Some(items) = taboos {
        if items.len() > 20 {
            errors.push("taboos: max 20 items, 1-200 chars each".to_string());
        }
    }
    let samples = obj.get("samples").and_then(Value::as_array);
    if let Some(items) = samples {
        if items.len() > 10 {
            errors.push("samples: max 10 items, 1-600 chars each".to_string());
        }
    }
    if !errors.is_empty() {
        return AssetValidateResult { asset: None, errors };
    }

    let strings = |field: &str| -> Vec<String> {
        obj.get(field)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    AssetValidateResult {
        asset: Some(LibraryAsset {
            id: id.to_string(),
            kind: kind.unwrap(),
            name: name.to_string(),
            body: body.to_string(),
            expectations: strings("expectations"),
            taboos: strings("taboos"),
            samples: strings("samples"),
        }),
        errors,
    }
}

const ARRAY_FIELD_SEP: &str = "\u{1f}";

/// 内容指纹的 canonical 形式（双端逐字一致；字段含 `|` 不影响确定性）。
pub fn asset_canonical_form(asset: &LibraryAsset) -> String {
    [
        asset.kind.as_str(),
        asset.id.as_str(),
        asset.name.as_str(),
        asset.body.as_str(),
        &asset.expectations.join(ARRAY_FIELD_SEP),
        &asset.taboos.join(ARRAY_FIELD_SEP),
        &asset.samples.join(ARRAY_FIELD_SEP),
    ]
    .join("|")
}

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv1a_hex16(text: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// 资产内容指纹（同内容必同指纹——幂等合并依据）。
pub fn asset_content_hash(asset: &LibraryAsset) -> String {
    fnv1a_hex16(&asset_canonical_form(asset))
}

/// 库稳定排序：kind 升序 → id 升序（纯码元比较）。
pub fn sort_assets(assets: &mut Vec<LibraryAsset>) {
    assets.sort_by(|a, b| a.kind.as_str().cmp(b.kind.as_str()).then_with(|| a.id.cmp(&b.id)));
}

/// 导出便携 JSON 包：稳定排序 + 固定键序（version 前置）。
pub fn build_asset_library_export(assets: &[LibraryAsset]) -> String {
    let mut sorted = assets.to_vec();
    sort_assets(&mut sorted);
    let payload = serde_json::json!({
        "version": ASSET_LIBRARY_VERSION,
        "assets": sorted,
    });
    serde_json::to_string_pretty(&payload).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetImportResult {
    pub assets: Vec<LibraryAsset>,
    /// 非法条目错误（index + 原因）。
    pub errors: Vec<String>,
}

/// 便携包导入：逐条校验，非法条目跳过记错误；版本不符整包拒绝；产物按库序排稳。
pub fn parse_asset_library_import(json: &str) -> AssetImportResult {
    let pkg: Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => {
            return AssetImportResult { assets: Vec::new(), errors: vec![format!("(json): {error}")] };
        }
    };
    if pkg.get("version").and_then(Value::as_i64) != Some(ASSET_LIBRARY_VERSION) {
        return AssetImportResult {
            assets: Vec::new(),
            errors: vec![format!("(version): expected {ASSET_LIBRARY_VERSION}")],
        };
    }
    let items = match pkg.get("assets").and_then(Value::as_array) {
        Some(items) => items,
        None => {
            return AssetImportResult { assets: Vec::new(), errors: vec!["(assets): expected array".to_string()] };
        }
    };
    let mut assets: Vec<LibraryAsset> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (index, raw) in items.iter().enumerate() {
        let result = validate_library_asset(raw);
        if let Some(asset) = result.asset {
            assets.push(asset);
        } else {
            for message in result.errors {
                errors.push(format!("assets[{index}] {message}"));
            }
        }
    }
    sort_assets(&mut assets);
    AssetImportResult { assets, errors }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMergeResult {
    pub merged: Vec<LibraryAsset>,
    /// 同 id 同内容跳过数（幂等）。
    pub skipped: usize,
    /// 同 id 异内容覆盖数（incoming 赢）。
    pub overwritten: usize,
    /// 新增数。
    pub added: usize,
}

/// 幂等合并：同 kind+id 同 hash 跳过；异 hash 覆盖（incoming 赢）；新 id 追加。
pub fn merge_asset_library(existing: &[LibraryAsset], incoming: &[LibraryAsset]) -> AssetMergeResult {
    let mut by_key: std::collections::BTreeMap<String, LibraryAsset> = existing
        .iter()
        .map(|asset| (format!("{}:{}", asset.kind.as_str(), asset.id), asset.clone()))
        .collect();
    let mut skipped = 0usize;
    let mut overwritten = 0usize;
    let mut added = 0usize;
    for asset in incoming {
        let key = format!("{}:{}", asset.kind.as_str(), asset.id);
        match by_key.get(&key) {
            Some(current) => {
                if asset_content_hash(current) == asset_content_hash(asset) {
                    skipped += 1;
                } else {
                    by_key.insert(key, asset.clone());
                    overwritten += 1;
                }
            }
            None => {
                by_key.insert(key, asset.clone());
                added += 1;
            }
        }
    }
    let mut merged: Vec<LibraryAsset> = by_key.into_values().collect();
    sort_assets(&mut merged);
    AssetMergeResult { merged, skipped, overwritten, added }
}

/// 内置精选题材基底种子（首批 3 个，随包分发；355 号空库冷启动对策）。
pub fn genre_base_seeds() -> Vec<LibraryAsset> {
    vec![
        LibraryAsset {
            id: "urban_supernatural".to_string(),
            kind: AssetKind::GenreBase,
            name: "都市异能".to_string(),
            body: "现代都市背景下的超凡力量体系。核心张力=日常身份与异能秘密的双线挤压：异能升级必须付出可感知代价（精力/人脉/隐匿风险），力量增长与生活崩坏同步推进。金手指要克制——前期异能只能解决「被欺负」而不是「翻身」，翻身要靠主角用异能做出现代人的聪明决策。".to_string(),
            expectations: vec![
                "每 3–5 章一次异能用法的新花样（不是数值变强而是用法变巧）".to_string(),
                "都市秩序（工作/家庭/朋友）持续被异能秘密挤压并付出代价".to_string(),
                "反派同样受现实规则约束，斗智成分不低于斗力".to_string(),
            ],
            taboos: vec![
                "异能万能化：解决一切问题的按键式金手指".to_string(),
                "现代人行为逻辑消失：主角获得力量后变成古代皇帝思维".to_string(),
                "反派降智：为了衬托主角而让对手做出违背利益的决策".to_string(),
            ],
            samples: vec![
                "他把手按在闸机上，电流顺着指尖爬进系统——余额清零的提示音响起时，保安的目光刚好扫过来。三秒。他只有三秒装作什么都没发生。".to_string(),
            ],
        },
        LibraryAsset {
            id: "xuanhuan_cultivation".to_string(),
            kind: AssetKind::GenreBase,
            name: "玄幻修真".to_string(),
            body: "境界驱动的东方玄幻。核心循环=资源争夺→境界突破→新地图新秩序：每次突破都要改写主角在势力格局中的位置，而不是只改数字。修炼体系须自洽（境界间有质变标志），突破节点放小高潮，突破代价与心魔埋进人物弧线。".to_string(),
            expectations: vec![
                "境界突破有可感知的质变标志（新能力/新视野/新敌人）".to_string(),
                "资源线清晰：灵石/功法/丹药争夺驱动至少一半章节冲突".to_string(),
                "每卷一个跨越势力层级的新格局（宗门→州→域→界）".to_string(),
            ],
            taboos: vec![
                "闭关流水账：跳过冲突的纯数值突破".to_string(),
                "境界碾压万能：低阶永远不可能凭智谋/底牌赢高阶".to_string(),
                "配角工具化：所有NPC只为供应主角资源而存在".to_string(),
            ],
            samples: vec![
                "丹成那一刻，雷云没有散——它们在等他的下一口气。林动笑了：夺舍他的老东西，恐怕没想到这一炉丹里掺了自己的血。".to_string(),
            ],
        },
        LibraryAsset {
            id: "mystery_investigation".to_string(),
            kind: AssetKind::GenreBase,
            name: "悬疑刑侦".to_string(),
            body: "信息差驱动的侦探叙事。核心引擎=读者与侦探的信息差管理：每章释放一条可回溯的线索，同时制造一个新的误导方向。案件结构=表面动机→隐藏动机→结构性真凶，破案靠证据链而非巧合；主角的执念/伤痕是贯穿案件外的第二主线。".to_string(),
            expectations: vec![
                "每章至少一条可回溯的真实线索（读者理论上可推出真相）".to_string(),
                "每案件三层反转：表象→动机→结构（最大意外落在结构层）".to_string(),
                "破案前主角付出真实代价（受伤/失去/立场崩塌）".to_string(),
            ],
            taboos: vec![
                "巧合破案：关键证据靠运气送上门".to_string(),
                "侦探全知：主角知道读者不可能知道的私密信息".to_string(),
                "真凶零铺垫：结局突然出现前文未存在的角色".to_string(),
            ],
            samples: vec![
                "档案袋里只有一张超市小票。老周看了三遍——收银员编号是 police 分局的内勤代码。买酸奶的时间，正是三年前那场大火的凌晨。".to_string(),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_and_rejects_shapes() {
        let good = serde_json::json!({
            "id": "three_act", "kind": "progression-mode", "name": "三幕推进", "body": "正文"
        });
        assert!(validate_library_asset(&good).asset.is_some());
        let bad_id = serde_json::json!({
            "id": "Urban-X", "kind": "genre-base", "name": "坏", "body": "正文"
        });
        let result = validate_library_asset(&bad_id);
        assert!(result.asset.is_none());
        assert!(result.errors[0].starts_with("id"));
    }

    #[test]
    fn hash_is_stable_across_calls() {
        let asset = genre_base_seeds().remove(0);
        assert_eq!(asset_content_hash(&asset), asset_content_hash(&asset));
    }
}
