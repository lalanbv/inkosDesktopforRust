import { describe, it, expect, afterEach, vi } from "vitest";
import { localizeTuiRuntimeMessage, localizeAgentFailure } from "../localization.js";
import { wrapLLMError } from "@actalk/inkos-core";

/**
 * 1057 号（1007 备案②清偿）：cli TUI 运行时错误消息 zh 承接接线活体。
 * 样例与 studio error-copy.test.ts 同源——均由 core wrapLLMError 生产模板
 * 派生（1007 号教训：映射正则↔生产字面锚定样例必须从生产函数导出，不可手打）。
 * 本测试锚定接线行为：TUI 显示路径（dashboard catch → setLastError）对
 * provider EN 诊断块按 CLI 语言承接——zh 映射回 zh 展示文案，en 保持 EN 原文。
 */
describe("localizeTuiRuntimeMessage (1057 TUI zh bridge wiring)", () => {
  afterEach(() => vi.unstubAllEnvs());

  const ctx = { baseUrl: "https://api.example.com/v1", model: "test-model" } as const;

  it("zh（INKOS_LOCALE=zh 显式）→ provider EN 诊断块映射回 zh（生产派生样例）", () => {
    vi.stubEnv("INKOS_LOCALE", "zh");
    const en = wrapLLMError(new Error("Request failed with status code 401"), ctx).message;
    const zh = localizeTuiRuntimeMessage(en);
    expect(zh).toContain("API 返回 401 (未授权)。请检查 .env 中的 INKOS_LLM_API_KEY 是否正确。");
    expect(zh).not.toContain("unauthorized");
  });

  it("en（INKOS_LOCALE=en 显式）→ 保持 EN 原文", () => {
    vi.stubEnv("INKOS_LOCALE", "en");
    const en = wrapLLMError(new Error("Request failed with status code 401"), ctx).message;
    expect(localizeTuiRuntimeMessage(en)).toBe(en);
  });

  it("5xx 无 detail 形态 → zh 整块承接（1007 号空格漂移回归锚，生产派生样例）", () => {
    vi.stubEnv("INKOS_LOCALE", "zh");
    // 与 studio error-copy.test.ts 同源派生形："(no body)" 触发无 detail 分支
    // （生产模板 "error). \n"——句点后有空格再换行，1007 号修正的高危面）。
    const en = wrapLLMError(new Error("Request failed with status code 500 (no body)"), ctx).message;
    expect(en).toContain("error). \nPossible causes:");
    const zh = localizeTuiRuntimeMessage(en);
    expect(zh).toContain("API 返回 5xx（上游服务异常）。\n可能原因：");
    expect(zh).not.toContain("上游详情");
  });

  it("未知消息 zh 下原样透传（非已知块不误伤）", () => {
    vi.stubEnv("INKOS_LOCALE", "zh");
    expect(localizeTuiRuntimeMessage("some unknown failure")).toBe("some unknown failure");
  });
});

/**
 * 1067 号（1057 清偿补全）：agent 链失败路径（errorMessage/completion.blocked
 * 捕获存状态面）的承接契约——undefined（无失败）透传、字符串走同款映射。
 * 消费点：tui/agent-input.ts / commands/interact.ts / commands/agent.ts。
 */
describe("localizeAgentFailure (1067 agent failure bridge)", () => {
  afterEach(() => vi.unstubAllEnvs());

  const ctx = { baseUrl: "https://api.example.com/v1", model: "test-model" } as const;

  it("undefined（无失败）透传为 undefined", () => {
    expect(localizeAgentFailure(undefined)).toBeUndefined();
  });

  it("provider EN 诊断块 → zh 承接（生产派生样例，与 TUI 抛异常路径同款）", () => {
    vi.stubEnv("INKOS_LOCALE", "zh");
    const en = wrapLLMError(new Error("Request failed with status code 429"), ctx).message;
    expect(localizeAgentFailure(en))
      .toContain("API 返回 429 (请求过多)。请稍后重试，或检查 API 配额。");
  });
});
