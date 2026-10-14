import { mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * R44/566 号（689 号随 2.0 合并重施）：skills:change 失效事件——import/delete
 * 后经 /api/v1/events 广播，UI 打开中的技能面板热刷新（useSkillsInvalidation）。
 * 断言机制移植自 fork skills-endpoint.test（582/583 号）SSE 帧读取器。
 */
describe("skills:change broadcast (R44/566, 689 重施)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "skills-change-"));
    await mkdir(join(root, ".inkos"), { recursive: true });
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("broadcasts skills:change with reason on import and delete over the events stream", async () => {
    const app = createStudioServer({} as never, root);
    // 连接事件流：handler 进 subscribers 后首个 ping 即流开启信号。
    const streamRes = await app.request("/api/v1/events");
    expect(streamRes.status).toBe(200);
    const reader = streamRes.body!.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    const waitEvent = async (name: string): Promise<string> => {
      for (;;) {
        const boundary = buffer.indexOf("\n\n");
        if (boundary >= 0) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          if (frame.includes(`event: ${name}`)) return frame;
          continue;
        }
        const { done, value } = await reader.read();
        if (done) throw new Error(`事件流结束仍未收到 ${name}`);
        buffer += decoder.decode(value, { stream: true });
      }
    };

    await waitEvent("ping"); // 流开启

    const manifest = [
      "---",
      "name: sse-probe",
      "description: 689 号广播断言探针。",
      "---",
      "正文",
    ].join("\n");
    const dataUrl = `data:text/markdown;base64,${Buffer.from(manifest, "utf8").toString("base64")}`;
    const imported = await app.request("/api/v1/skills/import", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ files: [{ path: "sse-probe/SKILL.md", dataUrl }] }),
    });
    expect(imported.status).toBe(200);

    const importFrame = await Promise.race([
      waitEvent("skills:change"),
      new Promise<never>((_, reject) => setTimeout(() => reject(new Error("import 未广播 skills:change")), 3000)),
    ]);
    expect(importFrame).toContain('"reason":"imported"');

    const removed = await app.request("/api/v1/skills/sse-probe", { method: "DELETE" });
    expect(removed.status).toBe(200);
    const deleteFrame = await Promise.race([
      waitEvent("skills:change"),
      new Promise<never>((_, reject) => setTimeout(() => reject(new Error("delete 未广播 skills:change")), 3000)),
    ]);
    expect(deleteFrame).toContain('"reason":"deleted"');

    await reader.cancel().catch(() => {});
  });
});
