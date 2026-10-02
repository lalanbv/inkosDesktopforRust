import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 589 提案 C（590 号）：零显式模型直发时解析链落到不含用户配置的默认卡，
 * 报「No API key for provider: openai」——secrets 里有可用服务时，错误消息
 * 必须改写为指认真实配置（消除「配置在场却被无视」的误导；行为不变：仍报错）。
 */
describe("agent no-model error annotates configured service (590)", () => {
  let root: string;

  const prepare = async () => {
    root = await mkdtemp(join(tmpdir(), "inkos-agent-hint-"));
    await mkdir(join(root, ".inkos"), { recursive: true });
    await writeFile(
      join(root, "inkos.json"),
      JSON.stringify({
        name: "hint-probe",
        version: "0.1.0",
        services: [{ service: "custom", name: "Mock", baseUrl: "http://127.0.0.1:9/v1" }],
      }),
    );
    await writeFile(
      join(root, ".inkos", "secrets.json"),
      JSON.stringify({ services: { "custom:Mock": { apiKey: "sk-mock" } } }),
    );
  };

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("annotates the no-key error with the configured service", async () => {
    await prepare();
    const app = createStudioServer({} as never, root);
    const created = await app.request("/api/v1/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ sessionKind: "chat" }),
    });
    const createdBody = (await created.json()) as { session: { sessionId: string } };
    const sessionId = createdBody.session.sessionId;

    const res = await app.request("/api/v1/agent", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ instruction: "你好", sessionId, sessionKind: "chat" }),
    });
    const body = (await res.json()) as { error?: { message?: string }; response?: string };

    expect(res.status).toBeGreaterThanOrEqual(400);
    const message = body.error?.message ?? body.response ?? "";
    expect(message).toContain("No API key for provider");
    expect(message).toContain("检测到已配置的服务「Mock」");
  });
});
