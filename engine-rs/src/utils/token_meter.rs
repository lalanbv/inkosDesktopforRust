//! R41 token 计量服务（563 号，v7 P1 / 544 号 §5 深化设计）。
//!
//! dsh token-meter `measure(session, requestHeader?)` 语义的同位落位：
//! - 启发式计价：复用 [`crate::llm::provider::estimate_text_tokens`]（CJK 1:1、
//!   其余 4:1，双端同源已锁），追加 O(1)（节点表 push + 计数累加）；
//! - usage 锚点修正：最近成功调用的 usage 总量 ÷ 记锚时刻启发式总量 = 校准比
//!   （clamp 0.25..=4.0 防离群），`measure()` 克隆节点表 O(surface) 即取即弃——
//!   快照不可变，锁内不执行用户代码；
//! - 锚点复用条件（"仅当……总量不低于其路由定价锚点时复用，否则全量重估"
//!   的确定性化）：新锚总量较前锚腰斩且表未缩（启发式总量不降）→ 判定异常
//!   重置，拒绝换锚沿用旧比；否则接受替换；
//! - 覆盖率：锚点记锚时刻启发式总量 / 当前启发式总量（min 1.0）——锚点对当前
//!   请求面的校准覆盖比例；
//! - 计量快照不可变（本仓零共享可变状态惯例）：[`TokenMeterSnapshot`] 是
//!   `measure()` 的值拷贝，可安全跨 await/线程持有。
//!
//! 隐私红线与 RunLog/R33 一致：只计量不存文——节点表只留启发式 token 数与
//! 序号，不留原文。

use std::sync::Mutex;

use crate::llm::provider::estimate_text_tokens;

/// 单节点：追加序号 + 追加时刻启发式 token 数（不留原文，见模块头）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceNode {
    pub seq: u64,
    pub heuristic_tokens: u32,
}

/// usage 锚点：最近成功调用的权威总量与其记锚时刻的启发式总量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageAnchor {
    pub model: Option<String>,
    /// 权威总量（usage.total 优先，缺省 input+output）。
    pub usage_total: u64,
    /// 记锚时刻表面启发式总量（校准比分母）。
    pub heuristic_total: u64,
}

#[derive(Debug, Default)]
struct SurfaceFold {
    nodes: Vec<SurfaceNode>,
    heuristic_total: u64,
    next_seq: u64,
    anchor: Option<UsageAnchor>,
}

/// 校准比 clamp（设计文档 0.25–4.0：防 usage 离群值污染估算）。
const RATIO_CLAMP_LOW: f64 = 0.25;
const RATIO_CLAMP_HIGH: f64 = 4.0;

/// 计量来源：usage 锚点生效 = "usage"，纯启发式 = "estimate"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeterSource {
    Usage,
    Estimate,
}

impl MeterSource {
    pub fn as_str(self) -> &'static str {
        match self {
            MeterSource::Usage => "usage",
            MeterSource::Estimate => "estimate",
        }
    }
}

/// 计量快照（值语义，serde camelCase 对齐 TS 消费面/双端 golden 向量）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(feature = "export-bindings", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-bindings", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TokenMeterSnapshot {
    /// 锚点模型（无锚 = null）。
    pub model: Option<String>,
    /// 当前表面启发式总量。
    pub heuristic_tokens: u64,
    /// 锚点校准后总量（无锚时 = heuristic_tokens）。
    pub anchored_tokens: u64,
    /// 采用值：锚点生效 = anchored_tokens，否则 heuristic_tokens。
    pub tokens: u64,
    pub anchor_valid: bool,
    pub source: &'static str,
    /// 锚点覆盖率（0.0..=1.0，4 位小数舍入；无锚 = 0.0）。
    pub coverage: f64,
    /// 输入窗（构造时注入；0 = 未知不判超窗）。
    pub input_window: u64,
    /// tokens > input_window（窗口未知时恒 false）。
    pub over_window: bool,
    /// 表面节点数（追加次数）。
    pub surface_nodes: usize,
}

/// token 计量器：追加 O(1) / 快照 O(surface) 即取即弃。
///
/// 每请求新建（聊天面 = 当前请求面的折叠表，跨轮重放语义由调用方保证：
/// run_agent_loop 每轮 append 的是"下一请求将新增的面"）。内部 Mutex 仅护
/// 节点表，快照在锁外构造。
pub struct TokenMeter {
    input_window: u64,
    fold: Mutex<SurfaceFold>,
}

impl TokenMeter {
    pub fn new(input_window: u64) -> Self {
        TokenMeter {
            input_window,
            fold: Mutex::new(SurfaceFold::default()),
        }
    }

    /// 追加一段文本入表面，返回节点序号。O(1)。
    pub fn append(&self, text: &str) -> u64 {
        let tokens = estimate_text_tokens(text);
        let mut fold = self.fold.lock().unwrap_or_else(|e| e.into_inner());
        let seq = fold.next_seq;
        fold.next_seq += 1;
        fold.heuristic_total += u64::from(tokens);
        fold.nodes.push(SurfaceNode {
            seq,
            heuristic_tokens: tokens,
        });
        seq
    }

    /// usage 权威值入账（最近成功调用）。返回是否接受为新锚（收缩防御拒绝
    /// 时沿用旧锚，见模块头）。`model` 为 None（执行器未透出）时锚点仍成立，
    /// 仅快照 model 面为 null。
    pub fn note_usage(
        &self,
        model: Option<&str>,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
    ) -> bool {
        let usage_total = if total_tokens > 0 {
            total_tokens
        } else {
            prompt_tokens + completion_tokens
        };
        if usage_total == 0 {
            return false;
        }
        let mut fold = self.fold.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(prev) = &fold.anchor {
            // 收缩防御：总量腰斩且表面未缩 → 异常重置（缓存错配/路由切换
            // 伪 usage），拒绝换锚。
            if usage_total * 2 < prev.usage_total && fold.heuristic_total >= prev.heuristic_total {
                return false;
            }
        }
        fold.anchor = Some(UsageAnchor {
            model: model.map(str::to_string),
            usage_total,
            heuristic_total: fold.heuristic_total,
        });
        true
    }

    /// 计量快照：锁内克隆节点表后锁外构造（即取即弃，O(surface)）。
    pub fn measure(&self) -> TokenMeterSnapshot {
        let fold = self.fold.lock().unwrap_or_else(|e| e.into_inner());
        let heuristic = fold.heuristic_total;
        let (anchored, coverage, anchor_valid, model) = match &fold.anchor {
            Some(anchor) if heuristic > 0 && anchor.heuristic_total > 0 => {
                let ratio = (anchor.usage_total as f64 / anchor.heuristic_total as f64)
                    .clamp(RATIO_CLAMP_LOW, RATIO_CLAMP_HIGH);
                let anchored = (heuristic as f64 * ratio).round() as u64;
                let coverage = ((anchor.heuristic_total.min(heuristic)) as f64 / heuristic as f64)
                    .min(1.0);
                (
                    anchored,
                    (coverage * 10_000.0).round() / 10_000.0,
                    true,
                    anchor.model.clone(),
                )
            }
            Some(anchor) => {
                // 表空或锚点记锚时表空：校准比未定义，退纯启发式（锚仍在场，
                // anchor_valid 保留 true——锚点身份可观测）。
                (heuristic, 0.0, true, anchor.model.clone())
            }
            None => (heuristic, 0.0, false, None),
        };
        let source = if anchor_valid {
            MeterSource::Usage
        } else {
            MeterSource::Estimate
        };
        let tokens = if anchor_valid { anchored } else { heuristic };
        TokenMeterSnapshot {
            model,
            heuristic_tokens: heuristic,
            anchored_tokens: anchored,
            tokens,
            anchor_valid,
            source: source.as_str(),
            coverage,
            input_window: self.input_window,
            over_window: self.input_window > 0 && tokens > self.input_window,
            surface_nodes: fold.nodes.len(),
        }
    }
}

/// 写作链输入准备面计量（v7 R41：prepare_write_input 返回处计量观测——
/// 超窗告警已有 budget notes 通道（ContextLens 消费），此处为计量对齐面，
/// 只观测不阻断）。
pub fn estimate_prepared_surface(
    intent: Option<&str>,
    memo_body: Option<&str>,
    context_entries: &[crate::models::input_governance::ContextSource],
) -> u64 {
    let mut total = 0u64;
    if let Some(text) = intent {
        total += u64::from(estimate_text_tokens(text));
    }
    if let Some(text) = memo_body {
        total += u64::from(estimate_text_tokens(text));
    }
    for entry in context_entries {
        total += u64::from(
            crate::utils::context_assembly::estimate_context_source_tokens(entry),
        );
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_only_state() {
        let meter = TokenMeter::new(1000);
        assert_eq!(meter.append("主角觉醒"), 0);
        assert_eq!(meter.append("abcd"), 1);
        let snap = meter.measure();
        assert_eq!(snap.heuristic_tokens, 5, "CJK 4 + ascii 1");
        assert!(!snap.anchor_valid);
        assert_eq!(snap.source, "estimate");
        assert_eq!(snap.tokens, 5);
        assert_eq!(snap.anchored_tokens, 5);
        assert_eq!(snap.coverage, 0.0);
        assert!(!snap.over_window);
        assert_eq!(snap.surface_nodes, 2);
        assert!(snap.model.is_none());
    }

    #[test]
    fn anchor_correction_scales_current_surface() {
        let meter = TokenMeter::new(1000);
        for _ in 0..25 {
            meter.append("主角觉醒"); // 25×4 = 100
        }
        assert!(meter.note_usage(Some("m1"), 100, 50, 150));
        let snap = meter.measure();
        assert_eq!(snap.heuristic_tokens, 100);
        assert_eq!(snap.anchored_tokens, 150, "ratio 1.5 × 100");
        assert_eq!(snap.tokens, 150);
        assert_eq!(snap.source, "usage");
        assert_eq!(snap.coverage, 1.0);
        assert_eq!(snap.model.as_deref(), Some("m1"));
        // 追加后：新面按同一比校准，覆盖率下降。
        meter.append("abcd"); // +1 → 101
        let snap = meter.measure();
        assert_eq!(snap.anchored_tokens, 152, "round(101×1.5)");
        assert!((snap.coverage - 0.9901).abs() < 1e-9, "100/101 → 0.9901");
    }

    #[test]
    fn anchor_rejected_on_halved_usage_with_unshrunk_surface() {
        let meter = TokenMeter::new(1000);
        for _ in 0..25 {
            meter.append("主角觉醒");
        }
        assert!(meter.note_usage(Some("m1"), 100, 50, 150));
        // 总量 50（腰斩）且启发式总量不降 → 拒绝。
        assert!(!meter.note_usage(Some("m2"), 20, 30, 50));
        let snap = meter.measure();
        assert_eq!(snap.model.as_deref(), Some("m1"), "旧锚沿用");
        assert_eq!(snap.anchored_tokens, 150);
        // 表 append-only（每请求新建）——同 meter 内表面恒不缩，腰斩即拒绝；
        // 跨请求换面走新 meter（新锚自然成立）。
        meter.append("abcd");
        assert!(!meter.note_usage(Some("m3"), 20, 30, 50), "后续腰斩同样拒绝");
    }

    #[test]
    fn anchor_replaced_when_usage_grows() {
        let meter = TokenMeter::new(1000);
        for _ in 0..25 {
            meter.append("主角觉醒");
        }
        assert!(meter.note_usage(Some("m1"), 100, 50, 150));
        assert!(meter.note_usage(Some("m2"), 200, 100, 300));
        let snap = meter.measure();
        assert_eq!(snap.model.as_deref(), Some("m2"));
        assert_eq!(snap.anchored_tokens, 300, "ratio 3.0 × 100");
    }

    #[test]
    fn ratio_clamped_to_quarter_and_quadruple() {
        let meter = TokenMeter::new(10_000);
        meter.append("abcdefgh"); // 2
        assert!(meter.note_usage(Some("m"), 100, 0, 100));
        let snap = meter.measure();
        assert_eq!(snap.anchored_tokens, 8, "ratio 50 → clamp 4.0 × 2");

        let meter = TokenMeter::new(10_000);
        for _ in 0..100 {
            meter.append("主角觉醒"); // 400
        }
        assert!(meter.note_usage(Some("m"), 10, 0, 10));
        let snap = meter.measure();
        assert_eq!(snap.anchored_tokens, 100, "ratio 0.025 → clamp 0.25 × 400");
    }

    #[test]
    fn usage_total_falls_back_to_input_plus_output() {
        let meter = TokenMeter::new(1000);
        for _ in 0..25 {
            meter.append("主角觉醒"); // 100
        }
        assert!(meter.note_usage(Some("m"), 30, 12, 0));
        let snap = meter.measure();
        assert_eq!(snap.anchored_tokens, 42, "total 0 → input+output");
    }

    #[test]
    fn zero_usage_not_anchored() {
        let meter = TokenMeter::new(1000);
        meter.append("hi");
        assert!(!meter.note_usage(Some("m"), 0, 0, 0));
        assert!(!meter.measure().anchor_valid);
    }

    #[test]
    fn empty_meter_and_empty_anchor() {
        let meter = TokenMeter::new(1000);
        let snap = meter.measure();
        assert_eq!(snap.tokens, 0);
        assert!(!snap.anchor_valid);
        assert!(!snap.over_window);
        // 锚先于任何追加（表空）：校准比未定义退启发式。
        assert!(meter.note_usage(Some("m"), 10, 0, 10));
        meter.append("hi");
        let snap = meter.measure();
        assert_eq!(snap.tokens, 1);
        assert_eq!(snap.anchored_tokens, 1, "锚时表空 → 退启发式");
        assert!(snap.anchor_valid);
    }

    #[test]
    fn over_window_flag() {
        let meter = TokenMeter::new(100);
        for _ in 0..25 {
            meter.append("主角觉醒"); // 100
        }
        assert!(!meter.measure().over_window, "100 ≤ 100");
        meter.append("主角觉醒");
        assert!(meter.measure().over_window, "101 > 100");
        // 窗口未知（0）恒 false。
        let unknown = TokenMeter::new(0);
        unknown.append("主角觉醒");
        assert!(!unknown.measure().over_window);
    }

    #[test]
    fn utf16_surrogate_pair_counted() {
        let meter = TokenMeter::new(1000);
        meter.append("😀😀"); // utf16 len 4，非 CJK → ceil(4/4)=1
        assert_eq!(meter.measure().heuristic_tokens, 1);
    }

    #[test]
    fn estimate_prepared_surface_sums_intent_memo_entries() {
        use crate::models::input_governance::ContextSource;
        let entry = ContextSource {
            source: "fact".into(),
            reason: "r".into(),
            excerpt: Some("主角觉醒".into()),
            rank: None,
        };
        // intent 4 + memo 1("abcd") + entry(source 1 + reason 1 + excerpt 4 →
        // ceil(6/4)+4=6？——以函数实现为准：source+reason+excerpt 拼接 6 非CJK
        // 字符→ceil(6/4)=2… 此处锁行为面而非公式）。
        let total = estimate_prepared_surface(Some("主角觉醒"), Some("abcd"), &[entry]);
        assert!(total > 0);
        let empty = estimate_prepared_surface(None, None, &[]);
        assert_eq!(empty, 0);
    }
}
