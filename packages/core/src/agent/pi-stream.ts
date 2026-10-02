// R30b 正统化：streamSimple 自持分发（llm/pi-dispatch.ts），替代 0.87
// deprecated 的 ./compat 子路径。
import { piStreamSimple, piCompleteSimple } from "../llm/pi-dispatch.js";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import type {
  Api,
  AssistantMessage,
  AssistantMessageEventStream,
  Context,
  Model,
  SimpleStreamOptions,
} from "@earendil-works/pi-ai";
import {
  assertWithinContextWindow,
  estimatePiContextTokens,
  guardAssistantMessageStream,
} from "../llm/provider.js";
import {
  agentTrajectoryHeaders,
  beginAgentModelCall,
} from "../llm/agent-trajectory.js";
import { isLlmStubEnabled, stubAgentStream } from "./llm-stub.js";

/**
 * The single Pi transport boundary used by both conversational and worker
 * agents. Pi keeps native tool calls; InkOS adds context guards, trajectory
 * headers, cancellation, and stream deadlines around the request.
 */
export function guardedPiStream<TApi extends Api>(
  model: Model<TApi>,
  context: Context,
  options?: SimpleStreamOptions,
): AssistantMessageEventStream {
  const reservedOutputTokens = Number.isFinite(options?.maxTokens)
    ? options!.maxTokens!
    : Number.isFinite(model.maxTokens)
      ? model.maxTokens
      : 4096;
  assertWithinContextWindow({
    piModel: model,
    model: model.id,
    estimatedInputTokens: estimatePiContextTokens(context),
    reservedOutputTokens,
  });
  const modelCall = beginAgentModelCall();
  const traceHeaders = agentTrajectoryHeaders(model.baseUrl, modelCall, 1, {
    effort: String(options?.reasoning ?? (model.reasoning ? "enabled" : "disabled")),
  });
  return guardAssistantMessageStream(
    model,
    (signal) => piStreamSimple(model, context, {
      ...options,
      headers: { ...(options?.headers ?? {}), ...traceHeaders },
      signal,
    }),
    options?.signal,
  );
}

/**
 * R45/567 号 Provider 选择缝（dsh Service Definition + Provider + Consumer
 * 三角色的 TS 定型面）：agent 流面的唯一入口——stub（`INKOS_AGENT_LLM_STUB`）
 * 与 pi 真传输的选择收拢在此，Consumer（agent-session/worker）不再分支于
 * 提供方身份。角色边界：Service Definition=LLMConfig/端点解析
 * （service-resolver/providers lookup）；Provider=本函数背后的 pi 传输
 * （`guardedPiStream`）与 llm-stub（`stubAgentStream`）；Consumer=各 agent
 * 编排层——只依赖本缝声明的能力，不 import 任何具体提供方。
 */
export function guardedAgentStream<TApi extends Api>(
  model: Model<TApi>,
  context: Context,
  options?: SimpleStreamOptions,
): AssistantMessageEventStream {
  if (isLlmStubEnabled()) return stubAgentStream(model, context);
  return guardedPiStream(model, context, options);
}

/**
 * 607 号：服务 stream:false 偏好的非流式完成→单事件流适配。
 *
 * 某些端点流式不稳定（服务详情「流式响应」开关关闭即此语义，108 号）——
 * 此前聊天路径恒流式忽略该偏好。非流式完成经守卫（窗口断言/轨迹头）后
 * 一次推入完整 assistant 消息并收尾，UX 为等待完整回复（无打字机增量）。
 */
export function guardedCompleteStream<TApi extends Api>(
  model: Model<TApi>,
  context: Context,
  options?: SimpleStreamOptions,
): AssistantMessageEventStream {
  const stream = createAssistantMessageEventStream();
  const modelCall = beginAgentModelCall();
  const traceHeaders = agentTrajectoryHeaders(model.baseUrl, modelCall, 1, {
    effort: String(options?.reasoning ?? (model.reasoning ? "enabled" : "disabled")),
  });
  const reservedOutputTokens = Number.isFinite(options?.maxTokens)
    ? options!.maxTokens!
    : Number.isFinite(model.maxTokens)
      ? model.maxTokens
      : 4096;
  assertWithinContextWindow({
    piModel: model,
    model: model.id,
    estimatedInputTokens: estimatePiContextTokens(context),
    reservedOutputTokens,
  });
  queueMicrotask(async () => {
    let message: AssistantMessage;
    try {
      message = await piCompleteSimple(model, context, {
        ...options,
        headers: { ...(options?.headers ?? {}), ...traceHeaders },
      });
    } catch (error) {
      stream.push({ type: "error", reason: "error", error: error instanceof Error ? error.message : String(error) } as never);
      stream.end(({
        role: "assistant",
        content: [],
        api: model.api,
        provider: model.provider,
        model: model.id,
        usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
        stopReason: "error",
        timestamp: Date.now(),
      }) as AssistantMessage);
      return;
    }
    stream.push({ type: "done", reason: (message.stopReason ?? "stop") as "stop", message });
    stream.end(message);
  });
  return stream;
}
