import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  activeChainEvents,
  appendTranscriptEvent,
  readTranscriptEvents,
  transcriptPath,
} from "../interaction/session-transcript.js";
import { branchBookSession, projectSessionTree } from "../interaction/session-branch.js";
import {
  deriveBookSessionFromTranscript,
  restoreAgentMessagesFromTranscript,
} from "../interaction/session-transcript-restore.js";

/**
 * R36 会话树化在 Node 回退腿的对话面（1666 号 duel 复活活体抓出）：692 号重施
 * 只回座了 branch 端点与 /branches 读面——会话读面（derive）与模型回放（restore）
 * 仍扫全量事件，分支后弃用轮次照常出现；本文件把两者钉在 active 链上，与
 * engine-rs（derive / restore_committed_dialogue_scan 入口先做 active 链过滤）
 * 同水位。纯 legacy 文件（无 branch_moved）过滤退化为全量——旧行为零改写。
 */
const usage = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

const SESSION = "s-tree";

/** 一个已提交回合：request_started → user → assistant(正文) → request_committed（4 个 seq）。 */
async function appendCommittedTurn(
  projectRoot: string,
  requestId: string,
  seq: number,
  input: string,
  reply: string,
): Promise<number> {
  const base = { type: "message" as const, version: 1 as const, sessionId: SESSION, requestId, parentUuid: null };
  await appendTranscriptEvent(projectRoot, {
    type: "request_started", version: 1, sessionId: SESSION, requestId, seq, timestamp: seq, input,
  });
  await appendTranscriptEvent(projectRoot, {
    ...base, uuid: `${requestId}-user`, seq: seq + 1, timestamp: seq + 1, role: "user",
    message: { role: "user", content: input, timestamp: seq + 1 },
  });
  await appendTranscriptEvent(projectRoot, {
    ...base, uuid: `${requestId}-assistant`, seq: seq + 2, timestamp: seq + 2, role: "assistant",
    message: {
      role: "assistant", api: "openai-completions", provider: "openai", model: "test",
      stopReason: "stop", usage, timestamp: seq + 2, content: [{ type: "text", text: reply }],
    },
  });
  const committed = seq + 3;
  await appendTranscriptEvent(projectRoot, {
    type: "request_committed", version: 1, sessionId: SESSION, requestId, seq: committed, timestamp: committed,
  });
  return committed;
}

/** 一个未提交（中断/失败）的工具回合：request_started → user → assistant(toolCall) → toolResult。 */
async function appendInterruptedToolTurn(projectRoot: string, requestId: string, seq: number, input: string): Promise<void> {
  const base = { type: "message" as const, version: 1 as const, sessionId: SESSION, requestId, parentUuid: null };
  await appendTranscriptEvent(projectRoot, {
    type: "request_started", version: 1, sessionId: SESSION, requestId, seq, timestamp: seq, input,
  });
  await appendTranscriptEvent(projectRoot, {
    ...base, uuid: `${requestId}-user`, seq: seq + 1, timestamp: seq + 1, role: "user",
    message: { role: "user", content: input, timestamp: seq + 1 },
  });
  await appendTranscriptEvent(projectRoot, {
    ...base, uuid: `${requestId}-calls`, seq: seq + 2, timestamp: seq + 2, role: "assistant",
    message: {
      role: "assistant", api: "openai-completions", provider: "openai", model: "test",
      stopReason: "toolUse", usage, timestamp: seq + 2,
      content: [{ type: "toolCall", id: `${requestId}-call`, name: "workspace__read", arguments: { artifactId: "scene" } }],
    },
  });
  await appendTranscriptEvent(projectRoot, {
    ...base, uuid: `${requestId}-result`, seq: seq + 3, timestamp: seq + 3, role: "toolResult", toolCallId: `${requestId}-call`,
    message: {
      role: "toolResult", toolCallId: `${requestId}-call`, toolName: "workspace__read",
      content: [{ type: "text", text: "{}" }], isError: false, timestamp: seq + 3,
    },
  });
}

/**
 * 读面消息 → `[role, content]`。`head`/`branchCount` 由服务端路由层以
 * `projectSessionTree` 合并进 derive 响应（core derive 本体不带），此处取同一
 * 单源函数核对。
 */
const readFace = async (projectRoot: string) => {
  const session = await deriveBookSessionFromTranscript(projectRoot, SESSION);
  const tree = projectSessionTree(await readTranscriptEvents(projectRoot, SESSION));
  return {
    head: tree.head,
    branchCount: tree.branchCount,
    title: session?.title,
    messages: (session?.messages ?? []).map((message) => [message.role, message.content]),
  };
};

/** 模型回放 → `[role, 正文]`（正文块拼接；工具轮只看角色）。 */
const modelReplay = async (projectRoot: string) => {
  const restored = await restoreAgentMessagesFromTranscript(projectRoot, SESSION);
  return restored.map((message) => {
    const content = (message as { content?: unknown }).content;
    const text = typeof content === "string"
      ? content
      : Array.isArray(content)
        ? content.filter((part): part is { type: "text"; text: string } => (
          typeof part === "object" && part !== null && (part as { type?: unknown }).type === "text"
        )).map((part) => part.text).join("")
        : "";
    return [message.role, text] as const;
  });
};

const chainSeqs = async (projectRoot: string) =>
  activeChainEvents(await readTranscriptEvents(projectRoot, SESSION)).map((event) => event.seq);

describe("session active chain on the Node fallback leg (R36)", () => {
  let projectRoot: string;

  beforeEach(async () => {
    projectRoot = await mkdtemp(join(tmpdir(), "inkos-active-chain-"));
  });

  afterEach(async () => {
    await rm(projectRoot, { recursive: true, force: true });
  });

  it("分支后读面与模型回放只含 active 链；新轮接在分支点之后", async () => {
    const first = await appendCommittedTurn(projectRoot, "r1", 1, "第一问", "第一答");
    const second = await appendCommittedTurn(projectRoot, "r2", first + 1, "第二问", "第二答");
    const turnTwo = ["user", "第二问"] as const;
    const replyTwo = ["assistant", "第二答"] as const;
    expect((await readFace(projectRoot)).messages).toEqual([
      ["user", "第一问"], ["assistant", "第一答"], turnTwo, replyTwo,
    ]);

    // 分支回第一轮：弃用的第二轮从读面与模型回放中整体剪除；元数据面照常。
    expect(await branchBookSession(projectRoot, SESSION, first)).toEqual({ head: first, branchCount: 1 });
    expect(await chainSeqs(projectRoot)).toEqual([1, 2, 3, 4]);
    expect(await readFace(projectRoot)).toMatchObject({
      head: first, branchCount: 1, messages: [["user", "第一问"], ["assistant", "第一答"]],
    });
    expect(await modelReplay(projectRoot)).toEqual([["user", "第一问"], ["assistant", "第一答"]]);

    // 分支上的新轮：链 = 第一轮 + 新轮，第二轮不回灌。
    const third = await appendCommittedTurn(projectRoot, "r3", second + 2, "分支上的新问", "新答");
    expect(await chainSeqs(projectRoot)).toEqual([1, 2, 3, 4, second + 2, second + 3, second + 4, third]);
    const expected = [
      ["user", "第一问"], ["assistant", "第一答"], ["user", "分支上的新问"], ["assistant", "新答"],
    ];
    expect((await readFace(projectRoot)).messages).toEqual(expected);
    expect(await modelReplay(projectRoot)).toEqual(expected);

    // 弃用路径零删改：再分支回去，第二轮原样回来。
    expect(await branchBookSession(projectRoot, SESSION, second)).toEqual({ head: second, branchCount: 2 });
    expect((await readFace(projectRoot)).messages).toEqual([
      ["user", "第一问"], ["assistant", "第一答"], turnTwo, replyTwo,
    ]);
    expect(await modelReplay(projectRoot)).toEqual([
      ["user", "第一问"], ["assistant", "第一答"], turnTwo, replyTwo,
    ]);
  });

  it("弃用分支上的中断工具回合不再回放；留在链上的中断回合照旧保留", async () => {
    const first = await appendCommittedTurn(projectRoot, "r1", 1, "第一问", "第一答");
    const second = await appendCommittedTurn(projectRoot, "r2", first + 1, "第二问", "第二答");
    await appendInterruptedToolTurn(projectRoot, "broken", second + 1, "没跑完的请求");

    // 未分支：链上的中断回合保留已完成工具交换 + 历史边界说明（既有行为）。
    const interrupted = await modelReplay(projectRoot);
    expect(interrupted.slice(0, 4)).toEqual([
      ["user", "第一问"], ["assistant", "第一答"], ["user", "第二问"], ["assistant", "第二答"],
    ]);
    expect(interrupted.map(([role]) => role)).toEqual([
      "user", "assistant", "user", "assistant", "user", "assistant", "toolResult", "assistant",
    ]);
    expect(interrupted[4]).toEqual(["user", "没跑完的请求"]);

    // 分支回第二轮末尾：中断回合在弃用分支上——连其用户输入与历史说明一并剪除。
    await branchBookSession(projectRoot, SESSION, second);
    expect(await modelReplay(projectRoot)).toEqual([
      ["user", "第一问"], ["assistant", "第一答"], ["user", "第二问"], ["assistant", "第二答"],
    ]);

    // 再分支回中断回合末尾：它重新成为链尾，照旧回放。
    await branchBookSession(projectRoot, SESSION, second + 4);
    expect(await modelReplay(projectRoot)).toEqual(interrupted);
  });

  it("resetLeaf 清空对话链；元数据保持全局折叠", async () => {
    await appendTranscriptEvent(projectRoot, {
      type: "session_created", version: 1, sessionId: SESSION, seq: 0, timestamp: 0, bookId: "b1",
      title: "定名", createdAt: 0, updatedAt: 0,
    });
    const first = await appendCommittedTurn(projectRoot, "r1", 1, "第一问", "第一答");
    await appendCommittedTurn(projectRoot, "r2", first + 1, "第二问", "第二答");

    // toSeq 缺省/null = head 置空：此后的写入开新链根，此前全部弃用。
    expect(await branchBookSession(projectRoot, SESSION, null)).toEqual({ head: null, branchCount: 1 });
    expect(await chainSeqs(projectRoot)).toEqual([]);
    expect(await readFace(projectRoot)).toMatchObject({
      head: null, branchCount: 1, title: "定名", messages: [],
    });
    expect(await modelReplay(projectRoot)).toEqual([]);

    const fresh = await appendCommittedTurn(projectRoot, "r3", first + 6, "重开新问", "新答");
    // 新链根 = 重开后的首事件（parentSeq=null），此前 10 个事件（含 branch_moved）全部弃用。
    expect(await chainSeqs(projectRoot)).toEqual([first + 6, first + 7, first + 8, fresh]);
    const chain = activeChainEvents(await readTranscriptEvents(projectRoot, SESSION));
    expect((chain[0] as { parentSeq?: unknown }).parentSeq).toBeNull();
    expect((await readFace(projectRoot)).messages).toEqual([["user", "重开新问"], ["assistant", "新答"]]);
    expect((await readFace(projectRoot)).title).toBe("定名");
    expect(await modelReplay(projectRoot)).toEqual([["user", "重开新问"], ["assistant", "新答"]]);
  });

  it("纯 legacy 文件（无 parentSeq / branch_moved）读面与回放零改写；其上分支按线性链回溯", async () => {
    await mkdir(join(projectRoot, ".inkos", "sessions"), { recursive: true });
    const event = (type: string, seq: number, extra: Record<string, unknown>) =>
      JSON.stringify({ type, version: 1, sessionId: SESSION, seq, timestamp: seq, ...extra });
    const user = (requestId: string, seq: number, text: string) =>
      event("message", seq, { requestId, uuid: `${requestId}-user`, parentUuid: null, role: "user",
        message: { role: "user", content: text, timestamp: seq } });
    const assistant = (requestId: string, seq: number, text: string) =>
      event("message", seq, { requestId, uuid: `${requestId}-assistant`, parentUuid: null, role: "assistant",
        message: { role: "assistant", content: [{ type: "text", text }], timestamp: seq } });
    await writeFile(transcriptPath(projectRoot, SESSION), [
      event("request_started", 1, { requestId: "r1", input: "旧问一" }),
      user("r1", 2, "旧问一"),
      assistant("r1", 3, "旧答一"),
      event("request_committed", 4, { requestId: "r1" }),
      event("request_started", 5, { requestId: "r2", input: "旧问二" }),
      user("r2", 6, "旧问二"),
      assistant("r2", 7, "旧答二"),
      event("request_committed", 8, { requestId: "r2" }),
    ].join("\n") + "\n", "utf-8");

    const whole = [["user", "旧问一"], ["assistant", "旧答一"], ["user", "旧问二"], ["assistant", "旧答二"]];
    expect(await chainSeqs(projectRoot)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect((await readFace(projectRoot)).messages).toEqual(whole);
    expect(await modelReplay(projectRoot)).toEqual(whole);

    // legacy 行缺 parentSeq：父 = seq 序前一事件（线性语义），分支照常生效。
    await branchBookSession(projectRoot, SESSION, 4);
    expect(await chainSeqs(projectRoot)).toEqual([1, 2, 3, 4]);
    expect((await readFace(projectRoot)).messages).toEqual(whole.slice(0, 2));
    expect(await modelReplay(projectRoot)).toEqual(whole.slice(0, 2));
  });
});
