import { describe, it, expect, afterEach } from "vitest";
import { localizeKnownRuntimeMessage } from "../lib/error-copy";
import { setAppLanguage } from "../lib/app-language";
// 1007 号：样例改由 core wrapLLMError 生产模板派生（不再手打）。1006 号教训
// 「映射正则活体=生产同形样例（手写样例漂移则静默失效）」的自证实例：5xx 无
// detail 手打样例漏了模板中句点后的空格（生产形 "error). \n"），映射正则同漂，
// 测试假绿而生产永不命中。派生样例使「生产字面 ↔ 映射正则」任一侧漂移即红。
import { wrapLLMError } from "@actalk/inkos-core";

/**
 * 1006 号：provider.ts 六条 HTTP 诊断块 EN 基准化后的 zh 展示承接映射活体。
 * 每条映射以 wrapLLMError 派生的真实 EN 形态（与 provider.ts 生产字面同源，
 * 含动态段与 ctxLine）喂入并断言 zh 关键片段——映射正则若与生产字面漂移
 * （多一个标点/一个空格）即红。409 消息映射（1004 号）一并锚定。
 */
describe("localizeKnownRuntimeMessage (1006 provider diagnostics zh bridge)", () => {
  afterEach(() => setAppLanguage("zh"));

  // 与 provider.ts 调用点同形：wrapLLMError(err, errorCtx)（ctxLine 由 context 派生）。
  const ctx = { baseUrl: "https://api.example.com/v1", model: "test-model" } as const;

  it("400 块有上游详情段 → zh 整块承接（生产派生样例）", () => {
    const en = wrapLLMError(
      Object.assign(new Error("Request failed with status code 400"), {
        error: { type: "invalid_request_error", message: "temperature must be <= 2" },
      }),
      ctx,
    ).message;
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 400（请求参数错误）。上游详情：invalid_request_error: temperature must be <= 2。");
    expect(zh).toContain("常见原因：");
    expect(zh).toContain("消息格式不兼容");
    expect(zh).not.toContain("Common causes");
    // ctxLine 不参与映射、原样保留（展示层附加上下文不丢）。
    expect(zh).toContain("(baseUrl: https://api.example.com/v1, model: test-model)");
  });

  it("400 块无上游详情段 → 无孤立『上游详情：』前缀（生产派生样例）", () => {
    const en = wrapLLMError(new Error("Request failed with status code 400"), ctx).message;
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 400（请求参数错误）。常见原因：");
    expect(zh).not.toContain("上游详情");
  });

  it("403 块 → zh 整块承接（生产派生样例）", () => {
    const en = wrapLLMError(new Error("Request failed with status code 403"), ctx).message;
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 403 (请求被拒绝)。可能原因：");
    expect(zh).toContain("内容审查拦截");
    expect(zh).toContain("建议：用 inkos doctor");
  });

  it("401 / 429 块 → zh 承接（生产派生样例）", () => {
    const unauthorized = wrapLLMError(new Error("Request failed with status code 401"), ctx).message;
    expect(localizeKnownRuntimeMessage(unauthorized))
      .toContain("API 返回 401 (未授权)。请检查 .env 中的 INKOS_LLM_API_KEY 是否正确。");
    const tooMany = wrapLLMError(new Error("Request failed with status code 429"), ctx).message;
    expect(localizeKnownRuntimeMessage(tooMany))
      .toContain("API 返回 429 (请求过多)。请稍后重试，或检查 API 配额。");
  });

  it("连接失败块 → baseUrl 动态段保留（生产派生样例）", () => {
    const en = wrapLLMError(new Error("fetch failed"), ctx).message;
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("无法连接到 API 服务。可能原因：");
    expect(zh).toContain("baseUrl 地址不正确（当前：https://api.example.com/v1）");
    expect(zh).toContain("建议：检查 INKOS_LLM_BASE_URL");
  });

  it("5xx 块有上游详情段 → zh 承接（生产派生样例）", () => {
    const en = wrapLLMError(
      Object.assign(new Error("Request failed with status code 500 (no body)"), {
        body: { reason: "MODEL_NOT_AVAILABLE", message: "model not available" },
      }),
      ctx,
    ).message;
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 5xx（上游服务异常）。上游详情：MODEL_NOT_AVAILABLE: model not available。");
    expect(zh).not.toContain("Possible causes");
  });

  it("5xx 块无上游详情段 → zh 承接（生产派生样例，1007 号空格漂移回归锚）", () => {
    // 生产模板：`error). ${detail ? … : ""}\n`——无 detail 时句点后有空格再换行
    // （"error). \n"）。手打样例曾漏该空格致映射正则同漂假绿（1007 号修复）。
    const en = wrapLLMError(new Error("Request failed with status code 500 (no body)"), ctx).message;
    expect(en).toContain("error). \nPossible causes:");
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain("API 返回 5xx（上游服务异常）。\n可能原因：");
    expect(zh).not.toContain("上游详情");
  });

  it("5xx 块上游详情段含换行 → 仍整块承接（1007 号跨行捕获）", () => {
    // 上游 body message 理论可含换行（多段校验信息）；捕获组须跨行否则
    // zh 用户回落 EN 原文。
    const en = wrapLLMError(
      Object.assign(new Error("Request failed with status code 500 (no body)"), {
        body: { reason: "MODEL_NOT_AVAILABLE", message: "model not available\ncontact provider support" },
      }),
      ctx,
    ).message;
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain(
      "API 返回 5xx（上游服务异常）。上游详情：MODEL_NOT_AVAILABLE: model not available\ncontact provider support。",
    );
    expect(zh).toContain("可能原因：");
    expect(zh).not.toContain("Possible causes");
  });

  it("400 块上游详情段含换行 → 仍整块承接（1007 号跨行捕获）", () => {
    const en = wrapLLMError(
      Object.assign(new Error("Request failed with status code 400"), {
        error: { type: "invalid_request_error", message: "invalid parameter:\ntemperature must be <= 2" },
      }),
      ctx,
    ).message;
    const zh = localizeKnownRuntimeMessage(en);
    expect(zh).toContain(
      "API 返回 400（请求参数错误）。上游详情：invalid_request_error: invalid parameter:\ntemperature must be <= 2。",
    );
    expect(zh).not.toContain("Common causes");
  });

  it("409 忙闸消息（1004 号）→ zh 承接", () => {
    // 生产者 studio api/server.ts 单行字面（协议面 EN 基准）；单行短句漂移风险
    // 低，维持字面锚定（core wrapLLMError 不产出该消息，无法派生）。
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
