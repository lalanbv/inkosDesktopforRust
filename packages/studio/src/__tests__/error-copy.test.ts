import { describe, it, expect, afterEach } from "vitest";
import { localizeKnownRuntimeMessage } from "../lib/error-copy";
import { setAppLanguage } from "../lib/app-language";

/**
 * 1006 号：provider.ts 六条 HTTP 诊断块 EN 基准化后的 zh 展示承接映射活体。
 * 每条映射以真实 EN 形态（与 provider.ts 生产字面同形，含动态段）喂入并断言
 * zh 关键片段——映射正则若与生产字面漂移（多一个标点/一个空格）即红。
 * 409 消息映射（1004 号）一并锚定。
 */
describe("localizeKnownRuntimeMessage (1006 provider diagnostics zh bridge)", () => {
  afterEach(() => setAppLanguage("zh"));

  it("400 块有上游详情段 → zh 整块承接", () => {
    const en = "API returned 400 (invalid request parameters). Upstream details: temperature must be <= 2.\nCommon causes:\n  1. temperature / max_tokens exceed the model's constraints (e.g. Moonshot kimi-k2.X forces temperature=1)\n  2. Model name is incorrect or not available\n  3. Incompatible message format (some services do not support the system or developer role)\n  (baseUrl: https://x, model: m1)";
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 400（请求参数错误）。上游详情：temperature must be <= 2。");
    expect(zh).toContain("常见原因：");
    expect(zh).toContain("消息格式不兼容");
    expect(zh).not.toContain("Common causes");
  });

  it("400 块无上游详情段 → 无孤立『上游详情：』前缀", () => {
    const en = "API returned 400 (invalid request parameters). Common causes:\n  1. a\n  2. b\n  3. c";
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 400（请求参数错误）。常见原因：");
    expect(zh).not.toContain("上游详情");
  });

  it("403 块 → zh 整块承接", () => {
    const en = "API returned 403 (request rejected). Possible causes:\n  1. API key is invalid or expired\n  2. The provider's content moderation blocked the request (common with free/community APIs)\n  3. Insufficient account balance\n  Suggestion: run inkos doctor to test API connectivity, or switch to a provider without content restrictions\n  (baseUrl: https://x, model: m1)";
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 403 (请求被拒绝)。可能原因：");
    expect(zh).toContain("内容审查拦截");
    expect(zh).toContain("建议：用 inkos doctor");
  });

  it("401 / 429 块 → zh 承接", () => {
    expect(localizeKnownRuntimeMessage("API returned 401 (unauthorized). Check INKOS_LLM_API_KEY in the project .env file."))
      .toContain("API 返回 401 (未授权)。请检查 .env 中的 INKOS_LLM_API_KEY 是否正确。");
    expect(localizeKnownRuntimeMessage("API returned 429 (too many requests). Retry later or check your API quota."))
      .toContain("API 返回 429 (请求过多)。请稍后重试，或检查 API 配额。");
  });

  it("连接失败块 → baseUrl 动态段保留", () => {
    const en = "Cannot connect to the API service. Possible causes:\n  1. Incorrect baseUrl (current: https://api.example.com/v2)\n  2. Network unreachable or blocked by a firewall\n  3. The API service is temporarily unavailable\n  Suggestion: check that INKOS_LLM_BASE_URL includes the full path (e.g. /v1)";
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("无法连接到 API 服务。可能原因：");
    expect(zh).toContain("baseUrl 地址不正确（当前：https://api.example.com/v2）");
    expect(zh).toContain("建议：检查 INKOS_LLM_BASE_URL");
  });

  it("5xx 块有/无上游详情段两型 → zh 承接", () => {
    const withDetail = localizeKnownRuntimeMessage("API returned 5xx (upstream service error). Upstream details: MODEL_NOT_AVAILABLE: model not available.\nPossible causes:\n  1. a\n  2. b\n  3. c");
    expect(withDetail).toContain("API 返回 5xx（上游服务异常）。上游详情：MODEL_NOT_AVAILABLE: model not available。");
    const withoutDetail = localizeKnownRuntimeMessage("API returned 5xx (upstream service error).\nPossible causes:\n  1. a\n  2. b\n  3. c");
    expect(withoutDetail).toContain("API 返回 5xx（上游服务异常）。\n可能原因：");
    expect(withoutDetail).not.toContain("上游详情");
  });

  it("409 忙闸消息（1004 号）→ zh 承接", () => {
    expect(localizeKnownRuntimeMessage("This session is still running. Wait for it to finish or stop it first."))
      .toBe("当前会话仍在运行，请等待完成或先停止。");
  });

  it("EN 模式 → 原样返回（映射仅在 zh/ja 生效）", () => {
    setAppLanguage("en");
    const en = "API returned 401 (unauthorized). Check INKOS_LLM_API_KEY in the project .env file.";
    expect(localizeKnownRuntimeMessage(en)).toBe(en);
  });

  it("未知消息 → 原样返回（真实失败不受扰）", () => {
    const raw = "Some unexpected upstream error:boom";
    expect(localizeKnownRuntimeMessage(raw)).toBe(raw);
  });
});
