//! 预测代理（agent.ts）：单次调用 + 校验驱动的一次重试。
//!
//! 首次响应未过 JSON/schema/分支数校验时，把错误回馈模型重试一次；二次
//! 失败为硬错误——runner 此时不写盘。

use crate::llm::agent_router::RoutedAgent;
use crate::llm::provider::{LLMMessage, LLMRole};

use super::prompts::{
    build_forecast_repair_prompt, build_forecast_system_prompt, build_forecast_user_prompt,
    ForecastPromptInput, ForecastLanguage,
};
use super::schema::{parse_forecast_model_output, ForecastModelOutput};

fn message(role: LLMRole, content: String) -> LLMMessage {
    LLMMessage { role, content, tool_calls: None, tool_call_id: None }
}

pub struct ForecastGenerationInput<'a> {
    pub context_markdown: &'a str,
    pub divergence: &'a str,
    pub branch_count: usize,
    pub horizon: u32,
    pub base_chapter: u32,
    pub language: &'a str,
}

/// 预测聊天端口（TS `BaseAgent.chat`——带 maxTokens 覆盖的最小面）。
#[async_trait::async_trait]
pub trait ForecastChat: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<String, String>;
}

#[async_trait::async_trait]
impl ForecastChat for RoutedAgent {
    async fn chat(
        &self,
        messages: Vec<LLMMessage>,
        temperature: f64,
        max_tokens: Option<u32>,
    ) -> Result<String, String> {
        self.router
            .chat(self.agent, messages, temperature, max_tokens)
            .await
            .map(|outcome| outcome.content)
    }
}

pub async fn generate_branches(
    chat: &dyn ForecastChat,
    input: &ForecastGenerationInput<'_>,
) -> Result<ForecastModelOutput, String> {
    let language: ForecastLanguage = if input.language == "en" { "en" } else { "zh" };
    let messages = vec![
        message(LLMRole::System, build_forecast_system_prompt(language)),
        message(
            LLMRole::User,
            build_forecast_user_prompt(
                &ForecastPromptInput {
                    context_markdown: input.context_markdown,
                    divergence: input.divergence,
                    branch_count: input.branch_count,
                    horizon: input.horizon,
                    base_chapter: input.base_chapter,
                },
                language,
            ),
        ),
    ];
    let max_tokens = estimate_forecast_max_tokens(input.branch_count, input.horizon);

    let first = chat.chat(messages.clone(), 0.6, Some(max_tokens)).await?;
    let first_error = match validate_generated_output(&first, input.branch_count) {
        Ok(output) => return Ok(output),
        Err(error) => error,
    };

    let mut retry_messages = messages;
    retry_messages.push(message(LLMRole::Assistant, first));
    retry_messages.push(message(
        LLMRole::User,
        build_forecast_repair_prompt(&first_error, language),
    ));
    let retry = chat.chat(retry_messages, 0.4, Some(max_tokens)).await?;
    validate_generated_output(&retry, input.branch_count)
}

fn validate_generated_output(raw: &str, expected_branches: usize) -> Result<ForecastModelOutput, String> {
    let output = parse_forecast_model_output(raw)?;
    if output.branches.len() != expected_branches {
        return Err(format!(
            "narrative forecast model returned {} branches, expected exactly {expected_branches}.",
            output.branches.len()
        ));
    }
    Ok(output)
}

/// 规划材料紧凑；按分支数与跨度留余量。zh 字符约 1.5 token/字，
/// 对 en 刻意超额配置。
fn estimate_forecast_max_tokens(branch_count: usize, horizon: u32) -> u32 {
    (8192usize.max(branch_count * (horizon as usize * 220 + 1600))) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ScriptChat {
        responses: Vec<String>,
        calls: std::sync::Mutex<Vec<(f64, Option<u32>)>>,
    }

    #[async_trait::async_trait]
    impl ForecastChat for ScriptChat {
        async fn chat(
            &self,
            _messages: Vec<LLMMessage>,
            temperature: f64,
            max_tokens: Option<u32>,
        ) -> Result<String, String> {
            self.calls.lock().unwrap().push((temperature, max_tokens));
            let index = self.calls.lock().unwrap().len() - 1;
            Ok(self.responses[index].clone())
        }
    }

    fn valid_output(branches: usize) -> String {
        let items: Vec<String> = (0..branches)
            .map(|i| {
                format!(
                    r#"{{"title":"分支{i}","premise":"p{i}","beats":[{{"chapter":6,"summary":"s"}}],"characterDecisions":[],"projectedChanges":{{"characters":[],"relationships":[],"world":[],"hooks":[]}},"risks":[],"uncertainties":[],"intentAlignment":{{"score":80,"rationale":"r"}}}}"#
                )
            })
            .collect();
        format!("{{\"branches\":[{}]}}", items.join(","))
    }

    #[tokio::test]
    async fn first_pass_success_no_retry() {
        let chat = ScriptChat {
            responses: vec![valid_output(3)],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let input = ForecastGenerationInput {
            context_markdown: "# ctx",
            divergence: "分歧",
            branch_count: 3,
            horizon: 5,
            base_chapter: 5,
            language: "zh",
        };
        let output = generate_branches(&chat, &input).await.unwrap();
        assert_eq!(output.branches.len(), 3);
        assert_eq!(chat.calls.lock().unwrap().len(), 1);
        // maxTokens = max(8192, 3*(5*220+1600)) = max(8192, 8100) = 8192。
        assert_eq!(chat.calls.lock().unwrap()[0].1, Some(8192));
    }

    #[tokio::test]
    async fn invalid_first_output_retries_once() {
        let chat = ScriptChat {
            responses: vec!["不是 JSON".to_string(), valid_output(2)],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let input = ForecastGenerationInput {
            context_markdown: "# ctx",
            divergence: "分歧",
            branch_count: 2,
            horizon: 5,
            base_chapter: 5,
            language: "zh",
        };
        let output = generate_branches(&chat, &input).await.unwrap();
        assert_eq!(output.branches.len(), 2);
        assert_eq!(chat.calls.lock().unwrap().len(), 2);
        // 重试降温 0.6 → 0.4。
        assert!((chat.calls.lock().unwrap()[1].0 - 0.4).abs() < 1e-9);
    }

    #[tokio::test]
    async fn branch_count_mismatch_is_error() {
        let chat = ScriptChat {
            responses: vec![valid_output(2), valid_output(4)],
            calls: std::sync::Mutex::new(Vec::new()),
        };
        let input = ForecastGenerationInput {
            context_markdown: "# ctx",
            divergence: "分歧",
            branch_count: 3,
            horizon: 5,
            base_chapter: 5,
            language: "zh",
        };
        let error = generate_branches(&chat, &input).await.unwrap_err();
        assert_eq!(
            error,
            "narrative forecast model returned 4 branches, expected exactly 3."
        );
    }
}
