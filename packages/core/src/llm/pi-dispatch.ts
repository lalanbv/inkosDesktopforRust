/**
 * pi-ai 传输分发（R30b 正统化，554 号）——deprecated `./compat` 子路径的
 * streamSimple/completeSimple 替代。compat 的分发链 = builtin provider 判定
 * → Models.applyAuth（凭据/OAuth/headers）→ api-registry → per-API 函数；
 * 本仓形态（无凭据库、model 自描述 baseUrl、options.apiKey 显式在场——
 * agent-loop 每 resolve getApiKey||config.apiKey 后透传，provider.ts 走
 * client._apiKey，compaction 走 secrets/env 自持解析）下前两层为空操作，
 * 故恒走 per-API 直接函数 + normalizeContext，行为等价。
 *
 * 与 compat 的差异（均备案）：builtin provider 分支（provider 级 OAuth/
 * ambient 注入，本仓全部自管 key）；cloudflare-ai-gateway 特判（本仓
 * provider 集不含）；withEnvApiKey env 兜底（三调用点显式 apiKey 在场，
 * env 兜底由 llm/pi-env-keys.ts 显式承接）；api-registry 自定义注册（本仓
 * 未使用）。dispatch 表只收本仓实际产生的 api 值，未知清晰抛错。
 */
import { normalizeContext } from "@earendil-works/pi-ai";
import type {
  Api,
  AssistantMessage,
  AssistantMessageEventStream,
  Context,
  Model,
  SimpleStreamOptions,
  TranscriptContext,
} from "@earendil-works/pi-ai";
import {
  streamSimple as openaiCompletionsStreamSimple,
} from "@earendil-works/pi-ai/api/openai-completions";
import {
  streamSimple as openaiResponsesStreamSimple,
} from "@earendil-works/pi-ai/api/openai-responses";
import {
  streamSimple as anthropicMessagesStreamSimple,
} from "@earendil-works/pi-ai/api/anthropic-messages";

type SimpleStreamFunction = (
  model: Model<Api>,
  context: TranscriptContext,
  options?: SimpleStreamOptions,
) => AssistantMessageEventStream;

// 各 per-API 模块的 streamSimple 为 StreamFunction<具体 Api>；本表按 model.api
// 键分发，键匹配即模型 api 匹配（与上游 api-registry 的泛型擦除同构），
// 故读取处单点断言收窄。
const API_STREAM_SIMPLE: Record<string, SimpleStreamFunction> = {
  "openai-completions": openaiCompletionsStreamSimple as SimpleStreamFunction,
  "openai-responses": openaiResponsesStreamSimple as SimpleStreamFunction,
  "anthropic-messages": anthropicMessagesStreamSimple as SimpleStreamFunction,
};

function dispatchStreamSimple(
  model: Model<Api>,
  context: Context,
  options?: SimpleStreamOptions,
): AssistantMessageEventStream {
  const streamSimple = API_STREAM_SIMPLE[model.api];
  if (!streamSimple) {
    throw new Error(`No pi-ai API module registered for api: ${model.api}`);
  }
  return streamSimple(model, normalizeContext(context), options);
}

/** compat `streamSimple` 替代：Context 归一化 + 按 model.api 直接分发。 */
export function piStreamSimple(
  model: Model<Api>,
  context: Context,
  options?: SimpleStreamOptions,
): AssistantMessageEventStream {
  return dispatchStreamSimple(model, context, options);
}

/** compat `completeSimple` 替代：流式发起后取完整 assistant 消息。 */
export async function piCompleteSimple(
  model: Model<Api>,
  context: Context,
  options?: SimpleStreamOptions,
): Promise<AssistantMessage> {
  return piStreamSimple(model, context, options).result();
}
