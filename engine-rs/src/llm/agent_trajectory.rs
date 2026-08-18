//! agent 轨迹遥测（TS `agent-trajectory.ts` 逐字）。
//!
//! 聊天回合作域内的 LLM 调用在 **kkaiapi.com 端点**上注入 `X-InkOS-*`
//! 观测头（会话/运行/调用标识 + 重试 attempt + 思考档位）；其它端点零头。
//! TS 用 AsyncLocalStorage 传播作用域；Rust 无同款运行时机制——以显式
//! `AgentTrajectoryScope`（Arc 共享 + pi_turn 原子递增）等价承载：
//! - 聊天面（RouterLoopChat）每回合构造 main 作用域；
//! - 管线面不挂作用域（TS 管线 agent 在作用域外 → beginAgentModelCall
//!   undefined → 零头，等价）；
//! - subagent 嵌套作用域结构已备（role/parent_tool_call_id），接线备案。

use sha2::{Digest, Sha256};

/// 单次 LLM 调用轨迹（TS `AgentModelCallTrace`——modelCallId 每次请求新造）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelCallTrace {
    pub conversation_id: String,
    pub run_id: String,
    pub model_call_id: String,
    pub agent_role: &'static str,
    pub pi_turn_index: u32,
    pub parent_tool_call_id: Option<String>,
}

/// 回合作用域（TS ALS store + piTurn 计数器的显式传递对应物）。
#[derive(Debug)]
pub struct AgentTrajectoryScope {
    pub conversation_id: String,
    pub run_id: String,
    pub agent_role: &'static str,
    pub parent_tool_call_id: Option<String>,
    /// main 角色每次 model call 递增（TS counter.piTurn）。Arc 共享——
    /// subagent 派生与父作用域同一计数器（TS `...current` 展开语义）。
    pi_turn: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl AgentTrajectoryScope {
    /// 聊天回合主作用域（TS runWithAgentTrajectory({role:"main"})）。
    pub fn main(conversation_id: String, run_id: String) -> Self {
        AgentTrajectoryScope {
            conversation_id,
            run_id,
            agent_role: "main",
            parent_tool_call_id: None,
            pi_turn: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }
    }

    /// TS `runWithAgentTrajectoryRole("subagent", task)`：继承本作用域
    ///（conversation/run/**同一计数器**），仅切 role——agent-tools 的调用
    /// 不传 parentToolCallId（头省略，逐字）。
    pub fn derive_subagent(&self) -> AgentTrajectoryScope {
        AgentTrajectoryScope {
            conversation_id: self.conversation_id.clone(),
            run_id: self.run_id.clone(),
            agent_role: "subagent",
            parent_tool_call_id: None,
            pi_turn: std::sync::Arc::clone(&self.pi_turn),
        }
    }

    /// TS `beginAgentModelCall`：取当前计数（main 时先递增，max(·,1)）+
    /// 新 modelCallId。
    pub fn begin_model_call(&self) -> AgentModelCallTrace {
        use std::sync::atomic::Ordering;
        let pi_turn = if self.agent_role == "main" {
            self.pi_turn.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            self.pi_turn.load(Ordering::SeqCst)
        };
        AgentModelCallTrace {
            conversation_id: self.conversation_id.clone(),
            run_id: self.run_id.clone(),
            model_call_id: uuid::Uuid::new_v4().to_string(),
            agent_role: self.agent_role,
            pi_turn_index: pi_turn.max(1),
            parent_tool_call_id: self.parent_tool_call_id.clone(),
        }
    }
}

/// TS `opaqueConversationId`：`inkos-` + sha256(sessionId) 十六进制前 32 位
/// （会话标识不透明化——不把原始 id 发给外部端点）。
pub fn opaque_conversation_id(session_id: &str) -> String {
    let digest = Sha256::digest(session_id.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("inkos-{}", &hex[..32])
}

/// TS `isKkaiapiEndpoint`：hostname 等于 kkaiapi.com 或以 .kkaiapi.com 结尾。
pub fn is_kkaiapi_endpoint(base_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(base_url) else {
        return false;
    };
    match parsed.host_str() {
        Some(host) => {
            let host = host.to_lowercase();
            host == "kkaiapi.com" || host.ends_with(".kkaiapi.com")
        }
        None => false,
    }
}

/// 思考档位（TS `ThinkingTrace`：effort 由 thinkingBudget>0 决定；Rust 端点
/// 配置无 thinkingBudget 机制——恒 disabled，budget 头省略，差异备案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkingTrace {
    pub effort: &'static str,
    pub budget_tokens: Option<u32>,
}

/// TS `agentTrajectoryHeaders`：仅 kkaiapi 端点发 9-11 个观测头；其它端点
/// 空集。Rust 无 withTransientLLMRetry 重试环——clientAttempt 恒 1（备案）。
pub fn agent_trajectory_headers(
    base_url: &str,
    trace: Option<&AgentModelCallTrace>,
    client_attempt: u32,
    thinking: ThinkingTrace,
) -> Vec<(String, String)> {
    let Some(trace) = trace else {
        return Vec::new();
    };
    if !is_kkaiapi_endpoint(base_url) {
        return Vec::new();
    }
    let mut headers: Vec<(String, String)> = vec![
        ("X-InkOS-Trace-Version".to_string(), "1".to_string()),
        ("X-InkOS-Scaffold".to_string(), "pi-inkos".to_string()),
        ("X-InkOS-Conversation-ID".to_string(), trace.conversation_id.clone()),
        ("X-InkOS-Run-ID".to_string(), trace.run_id.clone()),
        ("X-InkOS-Model-Call-ID".to_string(), trace.model_call_id.clone()),
        ("X-InkOS-Agent-Role".to_string(), trace.agent_role.to_string()),
        ("X-InkOS-Pi-Turn-Index".to_string(), trace.pi_turn_index.to_string()),
        ("X-InkOS-Client-Attempt".to_string(), client_attempt.to_string()),
        ("X-InkOS-Thinking-Effort".to_string(), thinking.effort.to_string()),
    ];
    if let Some(budget) = thinking.budget_tokens {
        headers.push((
            "X-InkOS-Thinking-Budget-Tokens".to_string(),
            budget.to_string(),
        ));
    }
    if let Some(parent) = &trace.parent_tool_call_id {
        headers.push((
            "X-InkOS-Parent-Tool-Call-ID".to_string(),
            parent.clone(),
        ));
    }
    headers
}

tokio::task_local! {
    /// 当前轨迹作用域（135 号：agent_route 回合 set main；sub_agent 工具
    /// 执行体 set 派生 subagent——同一 async 链内自动传播，等价 TS ALS）。
    pub static TRAJECTORY_SCOPE: Option<std::sync::Arc<AgentTrajectoryScope>>;
}

/// 读取当前作用域（无 task-local 或未 set → None——TS storage.getStore()
/// undefined 语义）。
pub fn current_scope() -> Option<std::sync::Arc<AgentTrajectoryScope>> {
    TRAJECTORY_SCOPE.try_with(|scope| scope.clone()).ok().flatten()
}

/// TS `runWithAgentTrajectoryRole("subagent", task)` 的 Rust 对应：
/// 外层无作用域则原样执行（`if (!current) return task()` 逐字）；有则
/// 派生 subagent 作用域包住执行体（共享 conversation/run/计数器）。
pub async fn with_subagent_scope<F>(task: F) -> F::Output
where
    F: std::future::Future,
{
    let derived = current_scope().map(|scope| std::sync::Arc::new(scope.derive_subagent()));
    match derived {
        Some(subagent) => TRAJECTORY_SCOPE.scope(Some(subagent), task).await,
        None => task.await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_id_is_prefixed_sha256_prefix() {
        let id = opaque_conversation_id("sess-1");
        assert!(id.starts_with("inkos-"), "{id}");
        assert_eq!(id.len(), "inkos-".len() + 32);
        // 确定性 + 不含原始 id。
        assert_eq!(id, opaque_conversation_id("sess-1"));
        assert_ne!(id, opaque_conversation_id("sess-2"));
    }

    #[test]
    fn kkaiapi_endpoint_detection() {
        assert!(is_kkaiapi_endpoint("https://kkaiapi.com/v1"));
        assert!(is_kkaiapi_endpoint("https://api.kkaiapi.com/v1"));
        assert!(!is_kkaiapi_endpoint("https://api.openai.com/v1"));
        assert!(!is_kkaiapi_endpoint("https://notkkaiapi.com/v1"), "后缀须为 .kkaiapi.com");
        assert!(!is_kkaiapi_endpoint("http://127.0.0.1:8080"));
        assert!(!is_kkaiapi_endpoint("not a url"));
    }

    #[test]
    fn headers_shape_matches_ts() {
        let trace = AgentModelCallTrace {
            conversation_id: "inkos-abc".into(),
            run_id: "run-1".into(),
            model_call_id: "call-1".into(),
            agent_role: "main",
            pi_turn_index: 2,
            parent_tool_call_id: Some("tool-9".into()),
        };
        let headers = agent_trajectory_headers(
            "https://api.kkaiapi.com/v1",
            Some(&trace),
            1,
            ThinkingTrace { effort: "disabled", budget_tokens: None },
        );
        let pairs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("X-InkOS-Trace-Version", "1"),
                ("X-InkOS-Scaffold", "pi-inkos"),
                ("X-InkOS-Conversation-ID", "inkos-abc"),
                ("X-InkOS-Run-ID", "run-1"),
                ("X-InkOS-Model-Call-ID", "call-1"),
                ("X-InkOS-Agent-Role", "main"),
                ("X-InkOS-Pi-Turn-Index", "2"),
                ("X-InkOS-Client-Attempt", "1"),
                ("X-InkOS-Thinking-Effort", "disabled"),
                ("X-InkOS-Parent-Tool-Call-ID", "tool-9"),
            ]
        );
        // 非 kkaiapi 端点零头；无 trace 零头；budget 条件头。
        assert!(agent_trajectory_headers(
            "https://api.openai.com/v1",
            Some(&trace),
            1,
            ThinkingTrace { effort: "disabled", budget_tokens: None },
        )
        .is_empty());
        assert!(agent_trajectory_headers(
            "https://api.kkaiapi.com/v1",
            None,
            1,
            ThinkingTrace { effort: "disabled", budget_tokens: None },
        )
        .is_empty());
        let with_budget = agent_trajectory_headers(
            "https://kkaiapi.com/v1",
            Some(&AgentModelCallTrace {
                parent_tool_call_id: None,
                ..trace
            }),
            1,
            ThinkingTrace { effort: "enabled", budget_tokens: Some(4096) },
        );
        assert!(with_budget.contains(&(
            "X-InkOS-Thinking-Budget-Tokens".to_string(),
            "4096".to_string()
        )));
        assert!(!with_budget
            .iter()
            .any(|(k, _)| k == "X-InkOS-Parent-Tool-Call-ID"));
    }

    #[tokio::test]
    async fn subagent_scope_shares_counter_and_never_increments() {
        let main = std::sync::Arc::new(AgentTrajectoryScope::main("inkos-x".into(), "run".into()));
        assert_eq!(main.begin_model_call().pi_turn_index, 1);
        // subagent 派生：同 conversation/run、计数器共享、parent 缺省。
        let sub = main.derive_subagent();
        assert_eq!(sub.conversation_id, "inkos-x");
        assert_eq!(sub.run_id, "run");
        assert_eq!(sub.agent_role, "subagent");
        assert!(sub.parent_tool_call_id.is_none(), "TS agent-tools 未传 parentToolCallId");
        let call = sub.begin_model_call();
        assert_eq!(call.pi_turn_index, 1, "subagent 读当前值不递增");
        assert_eq!(call.model_call_id.len(), 36, "每次调用新 uuid");
        // 回到 main：递增跨越 subagent 读取继续。
        assert_eq!(main.begin_model_call().pi_turn_index, 2);
    }

    #[tokio::test]
    async fn task_local_channel_propagates_and_subagent_wraps() {
        let main = std::sync::Arc::new(AgentTrajectoryScope::main("inkos-s".into(), "run-s".into()));
        assert!(current_scope().is_none(), "无 set 时 None");
        TRAJECTORY_SCOPE.scope(Some(main), async {
            assert_eq!(current_scope().unwrap().agent_role, "main");
            // subagent 包装：链内读到派生作用域；计数器共享。
            with_subagent_scope(async {
                let scope = current_scope().expect("包装内应有作用域");
                assert_eq!(scope.agent_role, "subagent");
                assert_eq!(scope.conversation_id, "inkos-s");
            })
            .await;
            // 包装外回 main。
            assert_eq!(current_scope().unwrap().agent_role, "main");
        })
        .await;
        // 无外层作用域时 with_subagent_scope 原样执行（不 panic）。
        with_subagent_scope(async {}).await;
    }

}
