//! R23 伏笔类型标注（393 号，四轮 P1；对标蛙趣 8 类伏笔生命周期的取精版）。
//!
//! TS 真源：`packages/core/src/utils/hook-kind.ts`。规范分类 ≤7 类不做
//! 数量战：承诺/悬念/危机/物品/信息/情感/世界观。自由文本经别名表精确
//! 归一化（未知 → None 绝不臆测）；双语展示标签。
//! 共享向量 `packages/core/src/__tests__/golden/hook-kind-vectors.json`
//! 差分：`tests/golden_hook_kind_diff.rs`。

use crate::models::runtime_state::HookKind;

pub const HOOK_KIND_IDS: [HookKind; 7] = [
    HookKind::Promise,
    HookKind::Suspense,
    HookKind::Crisis,
    HookKind::Artifact,
    HookKind::Information,
    HookKind::Emotion,
    HookKind::Worldview,
];

fn kind_id(kind: HookKind) -> &'static str {
    match kind {
        HookKind::Promise => "promise",
        HookKind::Suspense => "suspense",
        HookKind::Crisis => "crisis",
        HookKind::Artifact => "artifact",
        HookKind::Information => "information",
        HookKind::Emotion => "emotion",
        HookKind::Worldview => "worldview",
    }
}

/// 别名表查询（输入已小写化/trim；zh 别名原样，en 大小写不敏感）。
fn alias_lookup(key: &str) -> Option<HookKind> {
    Some(match key {
        // promise 承诺
        "promise" | "承诺" | "约定" | "誓言" => HookKind::Promise,
        // suspense 悬念
        "suspense" | "mystery" | "悬念" | "谜团" => HookKind::Suspense,
        // crisis 危机
        "crisis" | "危机" | "险局" | "威胁" => HookKind::Crisis,
        // artifact 物品
        "artifact" | "relic" | "物品" | "信物" | "道具" => HookKind::Artifact,
        // information 信息
        "information" | "info" | "secret" | "信息" | "情报" | "秘密" => HookKind::Information,
        // emotion 情感
        "emotion" | "relationship" | "romance" | "情感" | "感情" | "关系" => HookKind::Emotion,
        // worldview 世界观
        "worldview" | "lore" | "世界观" | "设定" => HookKind::Worldview,
        _ => return None,
    })
}

/// 自由文本 → 规范 kind；未知返回 None（绝不臆测归类）。
pub fn normalize_hook_kind(raw: &str) -> Option<HookKind> {
    alias_lookup(raw.trim().to_lowercase().as_str())
}

/// 规范 id 字符串（golden 向量与序列化对齐用）。
pub fn hook_kind_id(kind: HookKind) -> &'static str {
    kind_id(kind)
}

/// 双语展示标签（UI/timeline/回收提示）。
pub fn hook_kind_label(kind: HookKind, language: &str) -> &'static str {
    match language {
        "en" => match kind {
            HookKind::Promise => "Promise",
            HookKind::Suspense => "Suspense",
            HookKind::Crisis => "Crisis",
            HookKind::Artifact => "Artifact",
            HookKind::Information => "Information",
            HookKind::Emotion => "Emotion",
            HookKind::Worldview => "Worldview",
        },
        _ => match kind {
            HookKind::Promise => "承诺",
            HookKind::Suspense => "悬念",
            HookKind::Crisis => "危机",
            HookKind::Artifact => "物品",
            HookKind::Information => "信息",
            HookKind::Emotion => "情感",
            HookKind::Worldview => "世界观",
        },
    }
}
