import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { localizeKnownRuntimeMessage } from "@actalk/inkos-core/llm/error-copy";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 1669 号：「模型调用**之前**检出的 LLM 配置问题」→ 400 `LLM_CONFIG_ERROR` 信封
 * （与 Rust 腿同码同形）。
 *
 * 此前同一类问题三形态并存：冒泡到全局 onError 的（write-next / books/create）
 * 是 400 信封；处理器自带 try/catch 的（audit / revise / foundation / daemon）是
 * 500 裸串；`/agent` 是 500 `INKOS_ACTION_ERROR` 信封，缺 key 时更要等到请求
 * 发出才以模型错误失败。现由单源 helper（`llmConfigResponse`）归一。
 *
 * message 是 UI 文案表（core llm/error-copy）的逐字锚——末例锁住本地化仍命中。
 */
describe("LLM configuration problems detected before any model call (1669)", () => {
  let root: string;
  let app: ReturnType<typeof createStudioServer>;
  const J = { "Content-Type": "application/json" };
  const call = async (path: string, method = "POST", body?: unknown) => {
    const res = await app.request(path, { method, headers: J, body: body === undefined ? undefined : JSON.stringify(body) });
    return { status: res.status, body: (await res.json().catch(() => null)) as any };
  };

  /** 除 agent 外的模型调用面：此前 500 裸串（或冒泡到 onError 才有信封）。 */
  const ENVELOPE_FACES: ReadonlyArray<readonly [string, string, unknown]> = [
    ["write-next", "/api/v1/books/b1/write-next", {}],
    ["audit", "/api/v1/books/b1/audit/1", {}],
    ["revise", "/api/v1/books/b1/revise/1", { brief: "收紧节奏" }],
    ["foundation/revise", "/api/v1/books/b1/foundation/revise", { feedback: "收紧节奏" }],
    ["books/create", "/api/v1/books/create", { title: "新书", genre: "xuanhuan", platform: "qidian" }],
    ["daemon/start", "/api/v1/daemon/start", {}],
  ];

  const writeProjectConfig = async (llm: Record<string, unknown>) => {
    await writeFile(join(root, "inkos.json"), JSON.stringify({
      name: "llm-config", version: "0.1.0", language: "zh", llm,
    }), "utf-8");
  };
  const serviceConfig = (baseUrl: string) => ({
    service: "custom", provider: "openai", configSource: "studio", model: "m",
    services: [{ service: "custom", name: "Svc", models: ["m"], baseUrl, apiFormat: "chat", stream: false }],
    baseUrl, apiFormat: "chat", stream: false,
  });

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "llm-config-error-"));
    // 判定不得受宿主环境影响：清掉 env 层的 LLM 键。
    for (const key of ["INKOS_LLM_API_KEY", "INKOS_LLM_BASE_URL", "INKOS_LLM_MODEL", "INKOS_LLM_SERVICE"]) vi.stubEnv(key, "");
    const source = join(root, "works", "b1", "source");
    await mkdir(join(source, "chapters"), { recursive: true });
    await mkdir(join(source, "story", "outline"), { recursive: true });
    await writeFile(join(source, "book.json"), JSON.stringify({
      id: "b1", title: "配置契约书", platform: "qidian", genre: "xuanhuan", status: "active",
      targetChapters: 12, chapterWordCount: 3000, language: "zh",
      createdAt: "2026-10-16T00:00:00.000Z", updatedAt: "2026-10-16T00:00:00.000Z",
    }), "utf-8");
    await writeFile(join(source, "chapters", "0001_风起.md"), "# 风起\n\n正文。\n", "utf-8");
    await writeFile(join(source, "chapters", "index.json"), JSON.stringify([{
      number: 1, title: "风起", status: "ready-for-review", wordCount: 2,
      createdAt: "2026-10-16T00:00:00.000Z", updatedAt: "2026-10-16T00:00:00.000Z",
    }]), "utf-8");
    app = createStudioServer({} as never, root);
  });
  afterEach(async () => {
    vi.unstubAllEnvs();
    await rm(root, { recursive: true, force: true });
  });

  /** agent 面的检查在会话查找之后（顺序同改动前）——先建一个真实会话。 */
  const createSession = async () => {
    const created = await call("/api/v1/sessions", "POST", { sessionKind: "chat" });
    expect(created.status).toBe(200);
    return created.body.session.sessionId as string;
  };

  const expectEnvelope = async (fragment: string) => {
    for (const [name, path, body] of ENVELOPE_FACES) {
      const result = await call(path, "POST", body);
      expect(result, name).toEqual({ status: 400, body: { error: { code: "LLM_CONFIG_ERROR", message: expect.any(String) } } });
      expect(String(result.body.error.message), name).toContain(fragment);
    }
    // agent 面带 response（聊天气泡文本），其余同形。
    const agent = await call("/api/v1/agent", "POST", { instruction: "你好", sessionId: await createSession() });
    expect(agent.status).toBe(400);
    expect(agent.body.error.code).toBe("LLM_CONFIG_ERROR");
    expect(agent.body.response).toBe(agent.body.error.message);
    expect(String(agent.body.error.message)).toContain(fragment);
    // 失败发生在启动之前：没有留下后台任务。
    expect(await call("/api/v1/daemon/stop", "POST", {})).toMatchObject({ status: 400, body: { error: "Daemon not running" } });
    return String(agent.body.error.message);
  };

  it("no inkos.json: every model-calling face answers the same 400 envelope", async () => {
    await expectEnvelope("inkos.json not found");
  });

  it("a config without an API key: same code, and the message still hits the UI copy table", async () => {
    await writeProjectConfig(serviceConfig("https://example.invalid/v1"));
    const message = await expectEnvelope("Studio LLM API key not set");
    // 文案表逐字锚：改写 message（任一腿）即失去本地化。
    const localized = localizeKnownRuntimeMessage(message, "zh");
    expect(localized).not.toBe(message);
    expect(localized).toContain("模型配置");
  });

  it("key-optional endpoints (local / loopback) are not intercepted", async () => {
    await writeProjectConfig(serviceConfig("http://127.0.0.1:9/v1"));
    // 免 key 端点不拦；agent 的检查在会话查找之后——未知会话仍先报 404（顺序同
    // 改动前）。
    expect(await call("/api/v1/agent", "POST", { instruction: "你好", sessionId: "ghost" }))
      .toMatchObject({ status: 404, body: { error: { code: "SESSION_NOT_FOUND" } } });
  });

  it("confirmed production actions are not blocked by the chat-model key check", async () => {
    await writeProjectConfig(serviceConfig("https://example.invalid/v1"));
    const sessionId = await createSession();
    // 不走文本模型的确认式动作（generate_cover 走封面服务）：以自身的结果/错误
    // 收场，而不是被聊天模型的缺 key 误拦成 LLM_CONFIG_ERROR——首版把检查放在
    // 处理器开头，这类动作在「只配了封面服务」的项目上会被误拦。
    const result = await call("/api/v1/agent", "POST", {
      instruction: "生成封面", sessionId, actionSource: "button", requestedIntent: "generate_cover",
    });
    // 以自身的载荷校验错误收场（缺标题），不是 LLM_CONFIG_ERROR。
    expect(result).toEqual({
      status: 400,
      body: {
        error: { code: "CONFIRMED_ACTION_PAYLOAD_INCOMPLETE", message: "确认生成封面缺少标题，请重新生成确认卡。" },
        response: "确认生成封面缺少标题，请重新生成确认卡。",
      },
    });
  });

  it("failures that are not configuration problems keep their original shape", async () => {
    await writeProjectConfig(serviceConfig("https://example.invalid/v1"));
    // 非模型调用面：路径不存在的校验错误不被改写。
    expect(await call("/api/v1/books/b1/foundation/revise", "POST", {}))
      .toEqual({ status: 400, body: { error: "feedback is required" } });
  });
});
