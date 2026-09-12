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

    pub fn parse(value: &str) -> Option<AssetKind> {
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

/// 内置推进模式种子（363 号，双端逐字镜像；空库冷启动随包分发）。
pub fn progression_mode_seeds() -> Vec<LibraryAsset> {
    vec![
        LibraryAsset {
            id: "hook_cycle".to_string(),
            kind: AssetKind::ProgressionMode,
            name: "钩子循环推进".to_string(),
            body: "章级推进范式：章末强钩 → 下章开头真实承接 → 中段推进该钩并付出一次代价 → 章末再埋新钩。钩子类型轮换（悬念/危机/情感交替），循环以「代价被记账」为闭合标志——读者的期待是被承诺出来的账，每轮必须还一笔再欠一笔。".to_string(),
            expectations: vec![
                "章末钩与下章开头必须真实承接，不重置场景也不拖延兑现".to_string(),
                "钩子类型相邻循环不重复（悬念后接危机或情感）".to_string(),
                "每个循环内至少一次可感知代价（资源/关系/信息）".to_string(),
            ],
            taboos: vec![
                "同型钩子连用三章以上（读者疲劳点）".to_string(),
                "章末空抛：钩子内容与正文推进无关".to_string(),
                "只欠不还：连续多轮循环无任何旧钩兑付".to_string(),
            ],
            samples: vec![
                "她终于打开了那个盒子——里面的东西不是钱，是一张她自己签名的认罪书。日期，是明天。".to_string(),
            ],
        },
        LibraryAsset {
            id: "escalating_loop".to_string(),
            kind: AssetKind::ProgressionMode,
            name: "升级循环推进".to_string(),
            body: "卷级推进范式：每个循环（约 5–10 章）赌注明确升一级，且代价前置——先付出再收获。升级来自主角的选择与牺牲（用已知信息做更冒险的决定），而非外力送来的新外挂。上一轮的代价在下一轮持续生效成为本轮障碍，形成「债滚债」的推进压力。".to_string(),
            expectations: vec![
                "每循环开局一句话能说清本轮赌注比上轮大在哪".to_string(),
                "上轮代价在本轮至少一次实际阻碍主角".to_string(),
                "升级决策由主角主动做出并承担可见风险".to_string(),
            ],
            taboos: vec![
                "数值膨胀代替局势升级（敌人只是数字变大了）".to_string(),
                "危机重复同一形态（换个名字的同一件事）".to_string(),
                "外力救场：新外挂/新帮手凭空出现解决本轮危机".to_string(),
            ],
            samples: vec![
                "上次他赌上的是右手经脉。这一次，对面坐着全城最不该得罪的人，而他手里的筹码只有半张烧残的地图——和右手的旧伤。".to_string(),
            ],
        },
        LibraryAsset {
            id: "three_act".to_string(),
            kind: AssetKind::ProgressionMode,
            name: "三幕卷结构".to_string(),
            body: "卷级结构范式：建置（约 25%）→ 对抗（约 50%）→ 解决（约 25%），幕间各放一个转折点。第一转折打破主角的既有策略，中点用假胜利或假失败翻转局势，高潮同时解决主冲突并埋下卷间钩。三幕比例是节奏底线而非装饰——建置超四成必拖。".to_string(),
            expectations: vec![
                "第一转折落在卷内 20%–30% 处，且由主角自己的决定触发".to_string(),
                "中点有一次局势翻转（假胜利或假失败）".to_string(),
                "高潮解决本卷主冲突，同时开启下一卷的核心悬念".to_string(),
            ],
            taboos: vec![
                "建置超卷长四成（迟迟不进对抗幕）".to_string(),
                "转折无因果铺垫（纯意外事件砸脸）".to_string(),
                "解决幕拖尾：高潮后灌水超过卷长一成".to_string(),
            ],
            samples: vec![
                "所有人都以为庆功宴是这一卷的结束——直到主宾的椅子空了，桌上的信封里装着第三具尸体的照片。第一幕，才刚刚收尾。".to_string(),
            ],
        },
    ]
}

// ── R4/363 号：三库存储层（项目级 .inkos/asset-library/{kind}.json）──

fn seeds_for(kind: AssetKind) -> Vec<LibraryAsset> {
    match kind {
        AssetKind::GenreBase => genre_base_seeds(),
        AssetKind::ProgressionMode => progression_mode_seeds(),
        AssetKind::WorldSample => Vec::new(),
    }
}

pub fn library_file_path(project_root: &std::path::Path, kind: AssetKind) -> std::path::PathBuf {
    project_root
        .join(".inkos")
        .join("asset-library")
        .join(format!("{}.json", kind.as_str()))
}

/// 读取库快照：文件缺失/损坏/版本或 kind 不符 → 内置种子兜底（`seeded=true`，
/// 不自动落盘——用户数据文件只在首次写入时创建）。
pub fn list_assets(project_root: &std::path::Path, kind: AssetKind) -> std::io::Result<(Vec<LibraryAsset>, bool)> {
    let path = library_file_path(project_root, kind);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Ok((seeds_for(kind), true)),
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(_) => return Ok((seeds_for(kind), true)),
    };
    let version_ok = parsed.get("version").and_then(Value::as_i64) == Some(ASSET_LIBRARY_VERSION);
    let kind_ok = parsed
        .get("kind")
        .and_then(Value::as_str)
        .and_then(AssetKind::parse)
        .is_some_and(|parsed_kind| parsed_kind == kind);
    let items = parsed.get("assets").and_then(Value::as_array);
    if !version_ok || !kind_ok || items.is_none() {
        return Ok((seeds_for(kind), true));
    }
    let mut assets: Vec<LibraryAsset> = Vec::new();
    for item in items.unwrap_or(&Vec::new()) {
        if let Some(asset) = validate_library_asset(item).asset {
            assets.push(asset);
        }
    }
    sort_assets(&mut assets);
    Ok((assets, false))
}

/// 全量替换落盘（merge 幂等由调用方经 [`merge_asset_library`] 承担）。
pub fn save_assets(
    project_root: &std::path::Path,
    kind: AssetKind,
    assets: &[LibraryAsset],
) -> std::io::Result<()> {
    let path = library_file_path(project_root, kind);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut sorted = assets.to_vec();
    sort_assets(&mut sorted);
    let payload = serde_json::json!({
        "version": ASSET_LIBRARY_VERSION,
        "kind": kind.as_str(),
        "assets": sorted,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&payload).unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUpsertOutcome {
    pub kind: AssetKind,
    pub seeded: bool,
    pub skipped: usize,
    pub overwritten: usize,
    pub added: usize,
    pub merged: Vec<LibraryAsset>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// 单条资产 upsert：validate → merge（幂等）→ 落盘。
pub fn upsert_asset(
    project_root: &std::path::Path,
    kind: AssetKind,
    raw: &Value,
) -> AssetUpsertOutcome {
    let validation = validate_library_asset(raw);
    let Some(asset) = validation.asset else {
        return AssetUpsertOutcome {
            kind,
            seeded: false,
            skipped: 0,
            overwritten: 0,
            added: 0,
            merged: Vec::new(),
            errors: validation.errors,
        };
    };
    let (existing, seeded) = list_assets(project_root, kind).unwrap_or_else(|_| (Vec::new(), false));
    let merged = merge_asset_library(&existing, &[asset]);
    let outcome = AssetUpsertOutcome {
        kind,
        seeded: false,
        skipped: merged.skipped,
        overwritten: merged.overwritten,
        added: merged.added,
        merged: merged.merged.clone(),
        errors: Vec::new(),
    };
    match save_assets(project_root, kind, &merged.merged) {
        Ok(()) => outcome,
        Err(error) => AssetUpsertOutcome {
            kind,
            seeded,
            skipped: 0,
            overwritten: 0,
            added: 0,
            merged: existing,
            errors: vec![format!("(io): {error}")],
        },
    }
}

/// 删除资产：只作用于已落盘数据（内置种子不可删）。
pub fn delete_asset(
    project_root: &std::path::Path,
    kind: AssetKind,
    id: &str,
) -> Result<(bool, Option<String>, Vec<LibraryAsset>), String> {
    let (existing, seeded) = list_assets(project_root, kind).map_err(|error| error.to_string())?;
    if seeded {
        return Ok((false, Some("builtin seeds cannot be deleted; save the library first".to_string()), existing));
    }
    let remaining: Vec<LibraryAsset> = existing
        .iter()
        .filter(|asset| asset.id != id)
        .cloned()
        .collect();
    if remaining.len() == existing.len() {
        return Ok((false, Some(format!("asset not found: {id}")), existing));
    }
    save_assets(project_root, kind, &remaining).map_err(|error| error.to_string())?;
    Ok((true, None, remaining))
}

#[cfg(test)]
mod store_tests {
    use super::*;

    #[test]
    fn library_store_roundtrips_with_seed_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // 文件缺失 → 种子兜底。
        let (assets, seeded) = list_assets(root, AssetKind::GenreBase).unwrap();
        assert!(seeded);
        assert_eq!(assets.len(), 3);
        let (progression, seeded) = list_assets(root, AssetKind::ProgressionMode).unwrap();
        assert!(seeded);
        assert_eq!(progression.len(), 3);

        // upsert 覆盖种子（同 id 异内容）→ 落盘。
        let raw = serde_json::json!({
            "id": "urban_supernatural", "kind": "genre-base", "name": "都市异能", "body": "修订后的正文。"
        });
        let outcome = upsert_asset(root, AssetKind::GenreBase, &raw);
        assert_eq!(outcome.overwritten, 1, "{outcome:?}");
        let (assets, seeded) = list_assets(root, AssetKind::GenreBase).unwrap();
        assert!(!seeded);
        assert_eq!(assets.iter().find(|a| a.id == "urban_supernatural").unwrap().body, "修订后的正文。");

        // 幂等重放。
        let outcome = upsert_asset(root, AssetKind::GenreBase, &raw);
        assert_eq!(outcome.skipped, 1);

        // 删除：种子守卫 + 未找到 + 正常删除。
        let (ok, reason, _) = delete_asset(root, AssetKind::ProgressionMode, "hook_cycle").unwrap();
        assert!(!ok);
        assert!(reason.unwrap().contains("builtin seeds"));
        let (ok, reason, _) = delete_asset(root, AssetKind::GenreBase, "missing_id").unwrap();
        assert!(!ok);
        assert!(reason.unwrap().contains("not found"));
        let (ok, _, remaining) = delete_asset(root, AssetKind::GenreBase, "urban_supernatural").unwrap();
        assert!(ok);
        assert!(!remaining.iter().any(|a| a.id == "urban_supernatural"));
    }
}
