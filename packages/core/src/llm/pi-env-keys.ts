/**
 * pi-ai 环境变量 API key 解析（R30b 正统化自持副本，554 号）。
 *
 * 源 = @earendil-works/pi-ai 0.87 `dist/env-api-keys.js` 的 findEnvKeys /
 * getEnvApiKey——上游仅经 deprecated 的 `./compat` 子路径导出该模块（随
 * coding-agent ModelManager 迁移整体删除），本仓自持并以此处为单一事实源；
 * 上游升级漂移时 env-keys 表测试红，须考古后同步。
 *
 * 不移植（均备案）：google-vertex / amazon-bedrock 环境凭据特判（本仓
 * provider 集不含此二 provider）；Bun sandbox `/proc/self/environ` 回退
 * （Node 桌面形态 process.env 恒可用）；anthropic OAUTH_TOKEN 在上游
 * getEnvApiKey 中不跳过（仅跳过 AUTH_TOKEN），原样镜像不推理。
 */

const ANTHROPIC_AUTH_TOKEN_ENV = "ANTHROPIC_AUTH_TOKEN";
const ANTHROPIC_OAUTH_TOKEN_ENV = "ANTHROPIC_OAUTH_TOKEN";
const ANTHROPIC_API_KEY_ENV = "ANTHROPIC_API_KEY";

/** 上游 `getApiKeyEnvVars`：provider → 候选环境变量名（全表镜像）。 */
function getApiKeyEnvVars(provider: string): string[] | undefined {
  if (provider === "anthropic") {
    // AUTH_TOKEN 参与发现但不作为 key 返回（须以 Authorization: Bearer 传递）。
    return [ANTHROPIC_AUTH_TOKEN_ENV, ANTHROPIC_OAUTH_TOKEN_ENV, ANTHROPIC_API_KEY_ENV];
  }
  const envMap: Record<string, string> = {
    "ant-ling": "ANT_LING_API_KEY",
    "qwen-token-plan": "QWEN_TOKEN_PLAN_API_KEY",
    "qwen-token-plan-cn": "QWEN_TOKEN_PLAN_CN_API_KEY",
    "qwen-token-plan-individual": "QWEN_TOKEN_PLAN_API_KEY",
    openai: "OPENAI_API_KEY",
    "azure-openai-responses": "AZURE_OPENAI_API_KEY",
    nvidia: "NVIDIA_API_KEY",
    deepseek: "DEEPSEEK_API_KEY",
    google: "GEMINI_API_KEY",
    "google-vertex": "GOOGLE_CLOUD_API_KEY",
    groq: "GROQ_API_KEY",
    cerebras: "CEREBRAS_API_KEY",
    xai: "XAI_API_KEY",
    radius: "RADIUS_API_KEY",
    openrouter: "OPENROUTER_API_KEY",
    "vercel-ai-gateway": "AI_GATEWAY_API_KEY",
    zai: "ZAI_API_KEY",
    "zai-coding-cn": "ZAI_CODING_CN_API_KEY",
    mistral: "MISTRAL_API_KEY",
    minimax: "MINIMAX_API_KEY",
    "minimax-cn": "MINIMAX_CN_API_KEY",
    moonshotai: "MOONSHOT_API_KEY",
    "moonshotai-cn": "MOONSHOT_API_KEY",
    huggingface: "HF_TOKEN",
    fireworks: "FIREWORKS_API_KEY",
    together: "TOGETHER_API_KEY",
    baseten: "BASETEN_API_KEY",
    opencode: "OPENCODE_API_KEY",
    "opencode-go": "OPENCODE_API_KEY",
    "kimi-coding": "KIMI_API_KEY",
    meta: "META_API_KEY",
    "cloudflare-workers-ai": "CLOUDFLARE_API_KEY",
    "cloudflare-ai-gateway": "CLOUDFLARE_API_KEY",
    xiaomi: "XIAOMI_API_KEY",
    "xiaomi-token-plan-cn": "XIAOMI_TOKEN_PLAN_CN_API_KEY",
    "xiaomi-token-plan-ams": "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
    "xiaomi-token-plan-sgp": "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
  };
  const envVar = envMap[provider];
  return envVar ? [envVar] : undefined;
}

function getProviderEnvValue(name: string, env: Record<string, string> | undefined): string | undefined {
  return env?.[name] || (typeof process !== "undefined" ? process.env[name] : undefined) || undefined;
}

/**
 * 上游 `getEnvApiKey`：按 provider 候选表取首个有值的环境变量；
 * anthropic 跳过 AUTH_TOKEN。未命中返回 undefined。
 */
export function getEnvApiKey(provider: string, env?: Record<string, string>): string | undefined {
  const envVars = getApiKeyEnvVars(provider);
  if (!envVars) return undefined;
  const envKeys = envVars.filter((envVar) => !!getProviderEnvValue(envVar, env));
  if (envKeys.length === 0) return undefined;
  const apiKeyEnv = provider === "anthropic"
    ? envKeys.find((key) => key !== ANTHROPIC_AUTH_TOKEN_ENV)
    : envKeys[0];
  return apiKeyEnv ? getProviderEnvValue(apiKeyEnv, env) : undefined;
}
