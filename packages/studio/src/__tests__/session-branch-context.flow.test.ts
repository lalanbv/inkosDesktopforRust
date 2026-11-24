import { createServer } from "node:http";
import { once } from "node:events";
// 1015 号：测试凭据二轮治理——fixture 值走 env 注入+非凭据回退（855 惯用形二轮）。
const TEST_KEY = process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it } from "vitest";
import { evictAgentCache, readTranscriptEvents } from "@actalk/inkos-core";
import { createStudioServer } from "../api/server.js";

/**
 * R36 会话树化在 Node 回退腿的对话面（1666 号 duel 复活活体抓出）：分支后
 * `GET /sessions/:id` 只含 active 链，且下一轮的模型请求上下文也只带 active
 * 链。三处成因同测：会话读面扫全量事件 / 模型回放扫全量事件 / 缓存 Agent 的
 * 有效性指纹只看「全量最大提交 seq」——分支只追加 branch_moved，指纹不变，
 * 缓存 Agent 的内存上下文原样带着弃用轮次进入后续轮次。
 */
const TURNS = ["第一问", "第二问", "分支上的新问"] as const;

type ModelMessage = { role?: string; content?: unknown; tool_calls?: unknown };

it("keeps the session read face and the model context on the active chain across a branch", async () => {
  const root = await mkdtemp(join(tmpdir(), "inkos-branch-context-"));
  /** 每次模型请求的对话消息（system 除外）与其中命中的测试轮次（保序）。 */
  const requests: ModelMessage[][] = [];
  let calls = 0;
  let sessionId: string | undefined;
  const upstream = createServer(async (request, response) => {
    let raw = "";
    for await (const chunk of request) raw += chunk;
    calls++;
    const body = JSON.parse(raw) as { messages?: ModelMessage[] };
    requests.push((body.messages ?? []).filter((message) => message.role !== "system" && message.role !== "developer"));
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ choices: [{ finish_reason: "tool_calls", message: { role: "assistant", tool_calls: [{ id: `finish-${calls}`, type: "function",
      function: { name: "finish_turn", arguments: JSON.stringify({ status: "answered", message: "OK" }) } }] } }] }));
  });
  upstream.listen(0, "127.0.0.1"); await once(upstream, "listening");
  try {
    const baseUrl = `http://127.0.0.1:${(upstream.address() as { port: number }).port}/v1`;
    await mkdir(join(root, ".inkos"));
    await writeFile(join(root, "inkos.json"), JSON.stringify({ name: "fixture", version: "0.1.0", language: "zh", llm: {
      defaultModel: "fixture-model", services: [{ service: "custom", name: "fixture", baseUrl, apiFormat: "chat", stream: false, models: ["fixture-model"] }],
    } }));
    await writeFile(join(root, ".inkos/secrets.json"), JSON.stringify({ services: { "custom:fixture": { apiKey: TEST_KEY } } }));
    const app = createStudioServer({} as never, root);
    const post = (body: unknown) => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    const chat = async (instruction: string) => {
      const response = await app.request("/api/v1/agent", post({ sessionId, instruction, model: "fixture-model", service: "custom:fixture" }));
      expect(response.status, await response.clone().text()).toBe(200);
    };
    const readFace = async () => {
      const detail = await (await app.request(`/api/v1/sessions/${sessionId}`)).json() as {
        session: { head: number | null; branchCount: number; messages: Array<{ role: string; content: string }> };
      };
      return { head: detail.session.head, branchCount: detail.session.branchCount,
        messages: detail.session.messages.map((message) => `${message.role}:${message.content}`) };
    };
    const turnsSeen = (messages: ModelMessage[]) => TURNS.filter((turn) => messages.some((message) => JSON.stringify(message).includes(turn)));

    const { session } = await (await app.request("/api/v1/sessions", post({ sessionKind: "chat" }))).json();
    sessionId = session.sessionId;
    const turnOne = ["user:第一问", "assistant:OK"];
    const turnTwo = ["user:第二问", "assistant:OK"];
    const turnThree = ["user:分支上的新问", "assistant:OK"];

    await chat(TURNS[0]);
    await chat(TURNS[1]);
    expect(await readFace()).toEqual({ head: expect.any(Number), branchCount: 0, messages: [...turnOne, ...turnTwo] });
    // 对照：第二轮请求的上下文确实带着第一轮（历史回放在位，非恒空）。
    expect(requests).toHaveLength(2);
    expect(turnsSeen(requests[1]!)).toEqual([TURNS[0], TURNS[1]]);

    // 分支回第一轮末尾：读面只剩第一轮。
    const firstCommitted = (await readTranscriptEvents(root, sessionId!)).find((event) => event.type === "request_committed")!;
    const branch = await app.request(`/api/v1/sessions/${sessionId}/branch`, post({ toSeq: firstCommitted.seq }));
    expect(branch.status).toBe(200);
    expect(await readFace()).toEqual({ head: firstCommitted.seq, branchCount: 1, messages: turnOne });

    // 分支上的新轮：模型上下文只带 active 链（第一轮 + 新问），弃用的第二轮不回灌。
    await chat(TURNS[2]);
    expect(requests).toHaveLength(3);
    expect(turnsSeen(requests[2]!)).toEqual([TURNS[0], TURNS[2]]);
    expect(await readFace()).toEqual({ head: expect.any(Number), branchCount: 1, messages: [...turnOne, ...turnThree] });

    // 新轮事件接在分支点上（parentSeq = 分支目标），与弃用第二轮不连。
    const events = await readTranscriptEvents(root, sessionId!);
    const newTurnStart = events.find((event) => event.type === "request_started" && event.input === TURNS[2]);
    expect((newTurnStart as { parentSeq?: unknown } | undefined)?.parentSeq).toBe(firstCommitted.seq);
  } finally {
    if (sessionId) evictAgentCache(sessionId);
    upstream.closeAllConnections(); await new Promise<void>((resolve, reject) => upstream.close(error => error ? reject(error) : resolve()));
    await rm(root, { recursive: true, force: true });
  }
}, 20000);
