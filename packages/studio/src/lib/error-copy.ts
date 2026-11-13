import { getAppLanguage } from "./app-language";

const KNOWN_RUNTIME_REPLACEMENTS: ReadonlyArray<{
  readonly pattern: RegExp;
  readonly replacement: string;
}> = [
  {
    pattern: /Studio LLM API key not set\. Open Studio services and save an API key for the selected service\./g,
    replacement: "Studio 模型 API Key 未设置。请打开“模型配置”，为当前服务保存 API Key。",
  },
  {
    pattern: /INKOS_LLM_API_KEY not set\. Run 'inkos config set-global' or add it to project \.env file\./g,
    replacement: "INKOS_LLM_API_KEY 未设置。请运行 `inkos config set-global`，或在项目 .env 文件中添加它。",
  },
  {
    // 1004 号：CHAT_REQUEST_ALREADY_RUNNING 409 协议面转 EN（server.ts 对齐错误
    // 协议全 EN 族惯例）后，zh 会话内的展示由此映射承接。
    pattern: /This session is still running\. Wait for it to finish or stop it first\./g,
    replacement: "当前会话仍在运行，请等待完成或先停止。",
  },
  // ---- 1006 号：provider.ts 六条 HTTP 诊断块 EN 基准化后的 zh 展示承接 ----
  // 整块映射（动态段捕获组保留；有/无 detail 段分型）。「Upstream details: 」
  // 是跨端解析标记（engine-rs service_routes / studio server 双解析点认两形），
  // zh 展示映射回「上游详情：」。多行诊断块不被后续消费方二次解析（仅展示），
  // 行级 token 一一对应回 zh 原文。
  {
    pattern: /API returned 400 \(invalid request parameters\)\. Upstream details: ([^\n]*)\.\nCommon causes:\n  1\.[^\n]*\n  2\.[^\n]*\n  3\.[^\n]*/g,
    replacement: "API 返回 400（请求参数错误）。上游详情：$1。\n常见原因：\n  1. temperature / max_tokens 超出模型约束（如 Moonshot kimi-k2.X 强制 temperature=1）\n  2. 模型名称不正确或未上架\n  3. 消息格式不兼容（部分服务不支持 system role 或 developer role）",
  },
  {
    pattern: /API returned 400 \(invalid request parameters\)\. Common causes:\n  1\.[^\n]*\n  2\.[^\n]*\n  3\.[^\n]*/g,
    replacement: "API 返回 400（请求参数错误）。常见原因：\n  1. temperature / max_tokens 超出模型约束（如 Moonshot kimi-k2.X 强制 temperature=1）\n  2. 模型名称不正确或未上架\n  3. 消息格式不兼容（部分服务不支持 system role 或 developer role）",
  },
  {
    pattern: /API returned 403 \(request rejected\)\. Possible causes:\n  1\.[^\n]*\n  2\.[^\n]*\n  3\.[^\n]*\n  Suggestion:[^\n]*/g,
    replacement: "API 返回 403 (请求被拒绝)。可能原因：\n  1. API Key 无效或过期\n  2. API 提供方的内容审查拦截了请求（公益/免费 API 常见）\n  3. 账户余额不足\n  建议：用 inkos doctor 测试 API 连通性，或换一个不限制内容的 API 提供方",
  },
  {
    pattern: /API returned 401 \(unauthorized\)\. Check INKOS_LLM_API_KEY in the project \.env file\./g,
    replacement: "API 返回 401 (未授权)。请检查 .env 中的 INKOS_LLM_API_KEY 是否正确。",
  },
  {
    pattern: /API returned 429 \(too many requests\)\. Retry later or check your API quota\./g,
    replacement: "API 返回 429 (请求过多)。请稍后重试，或检查 API 配额。",
  },
  {
    pattern: /Cannot connect to the API service\. Possible causes:\n  1\. Incorrect baseUrl \(current: ([^\n]*)\)\n  2\.[^\n]*\n  3\.[^\n]*\n  Suggestion:[^\n]*/g,
    replacement: "无法连接到 API 服务。可能原因：\n  1. baseUrl 地址不正确（当前：$1）\n  2. 网络不通或被防火墙拦截\n  3. API 服务暂时不可用\n  建议：检查 INKOS_LLM_BASE_URL 是否包含完整路径（如 /v1）",
  },
  {
    pattern: /API returned 5xx \(upstream service error\)\. Upstream details: ([^\n]*)\.\nPossible causes:\n  1\.[^\n]*\n  2\.[^\n]*\n  3\.[^\n]*/g,
    replacement: "API 返回 5xx（上游服务异常）。上游详情：$1。\n可能原因：\n  1. 模型在 /models 列表但 inference 未上架（如 PPIO 返回 MODEL_NOT_AVAILABLE）\n  2. 服务端临时故障，稍后重试\n  3. 当前 apikey 无权限调用该模型",
  },
  {
    // 5xx 无 detail 段分型。
    pattern: /API returned 5xx \(upstream service error\)\.\nPossible causes:\n  1\.[^\n]*\n  2\.[^\n]*\n  3\.[^\n]*/g,
    replacement: "API 返回 5xx（上游服务异常）。\n可能原因：\n  1. 模型在 /models 列表但 inference 未上架（如 PPIO 返回 MODEL_NOT_AVAILABLE）\n  2. 服务端临时故障，稍后重试\n  3. 当前 apikey 无权限调用该模型",
  },
];

export function localizeKnownRuntimeMessage(message: string): string {
  // Runtime messages arrive in English; in English mode show them as-is.
  if (getAppLanguage() === "en") return message;
  let localized = message;
  for (const entry of KNOWN_RUNTIME_REPLACEMENTS) {
    localized = localized.replace(entry.pattern, entry.replacement);
  }
  return localized;
}
