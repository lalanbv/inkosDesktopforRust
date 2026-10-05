import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { mkdtemp, rm, mkdir, writeFile, readFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  activeChainEvents,
  appendTranscriptEvents,
  readTranscriptEvents,
  transcriptHead,
  transcriptPath,
} from "../interaction/session-transcript.js";
import {
  BranchTargetNotFoundError,
  branchBookSession,
} from "../interaction/book-session-store.js";
import {
  deriveBookSessionFromTranscript,
  restoreAgentMessagesFromTranscript,
} from "../interaction/session-transcript-restore.js";
import { TranscriptEventSchema } from "../interaction/session-transcript-schema.js";
import type { TranscriptEvent } from "../interaction/session-transcript-schema.js";

/** 一轮已提交对话：request_started + user + assistant + request_committed。 */
async function appendCommittedRound(
  projectRoot: string,
  sessionId: string,
  requestId: string,
  userText: string,
  assistantText: string,
): Promise<{ userUuid: string }> {
  const now = Date.now();
  const userUuid = `u-${requestId}-user`;
  await appendTranscriptEvents(projectRoot, sessionId, ({ nextSeq }) => {
    let seq = nextSeq;
    const events: TranscriptEvent[] = [
      {
        type: "request_started",
        version: 1,
        sessionId,
        requestId,
        seq: seq++,
        timestamp: now,
        input: userText,
      },
      {
        type: "message",
        version: 1,
        sessionId,
        requestId,
        uuid: userUuid,
        parentUuid: null,
        seq: seq++,
        role: "user",
        timestamp: now,
        message: { role: "user", content: userText, timestamp: now },
      },
      {
        type: "message",
        version: 1,
        sessionId,
        requestId,
        uuid: `u-${requestId}-assistant`,
        parentUuid: userUuid,
        seq: seq++,
        role: "assistant",
        timestamp: now,
        message: {
          role: "assistant",
          content: [{ type: "text", text: assistantText }],
          timestamp: now,
        },
      },
      {
        type: "request_committed",
        version: 1,
        sessionId,
        requestId,
        seq: seq++,
        timestamp: now,
      },
    ];
    return events;
  });
  return { userUuid };
}

async function restoredTexts(
  projectRoot: string,
  sessionId: string,
): Promise<string[]> {
  const messages = await restoreAgentMessagesFromTranscript(projectRoot, sessionId);
  return messages
    .filter((message) => (message as { role?: string }).role !== "system")
    .map((message) => {
      const raw = message as { role?: string; content?: unknown };
      if (raw.role === "user" && typeof raw.content === "string") return raw.content;
      if (Array.isArray(raw.content)) {
        return raw.content
          .map((block) => (block as { text?: string })?.text ?? "")
          .join("");
      }
      return typeof raw.content === "string" ? raw.content : "";
    });
}

describe("session branch（R36 会话树化）", () => {
  let projectRoot: string;

  beforeEach(async () => {
    projectRoot = await mkdtemp(join(tmpdir(), "inkos-branch-"));
  });

  afterEach(async () => {
    await rm(projectRoot, { recursive: true, force: true });
  });

  it("append 助手统一戳记 parentSeq：线性连续链（首个事件 parentSeq=null）", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    const events = await readTranscriptEvents(projectRoot, "s1");
    expect(events.map((event) => event.seq)).toEqual([1, 2, 3, 4]);
    expect(events[0].parentSeq).toBeNull();
    expect(events[1].parentSeq).toBe(1);
    expect(events[2].parentSeq).toBe(2);
    expect(events[3].parentSeq).toBe(3);
    expect(transcriptHead(events)).toBe(4);
    expect(activeChainEvents(events)).toHaveLength(4);
  });

  it("legacy 旧文件零改写解析：无 parentSeq 键的行照常解析、恢复为全量线性", async () => {
    const dir = join(projectRoot, ".inkos", "sessions");
    await mkdir(dir, { recursive: true });
    // 直接写 raw JSONL（绕过 append 助手）——模拟 R36 之前的旧文件形态。
    const legacyLines = [
      { type: "request_started", version: 1, sessionId: "s1", requestId: "r1", seq: 1, timestamp: 1, input: "旧一问" },
      { type: "message", version: 1, sessionId: "s1", requestId: "r1", uuid: "lu1", parentUuid: null, seq: 2, role: "user", timestamp: 1, message: { role: "user", content: "旧一问", timestamp: 1 } },
      { type: "message", version: 1, sessionId: "s1", requestId: "r1", uuid: "lu2", parentUuid: "lu1", seq: 3, role: "assistant", timestamp: 1, message: { role: "assistant", content: [{ type: "text", text: "旧一答" }], timestamp: 1 } },
      { type: "request_committed", version: 1, sessionId: "s1", requestId: "r1", seq: 4, timestamp: 1 },
    ];
    await writeFile(
      transcriptPath(projectRoot, "s1"),
      legacyLines.map((line) => JSON.stringify(line)).join("\n") + "\n",
      "utf-8",
    );
    const events = await readTranscriptEvents(projectRoot, "s1");
    expect(events).toHaveLength(4);
    expect(events.every((event) => event.parentSeq === undefined)).toBe(true);
    expect(await restoredTexts(projectRoot, "s1")).toEqual(["旧一问", "旧一答"]);

    // 旧文件上继续追加（新写入恒带值）：链从 legacy 前缀自然延展。
    await appendCommittedRound(projectRoot, "s1", "r2", "新二问", "新二答");
    const after = await readTranscriptEvents(projectRoot, "s1");
    expect(after[4].parentSeq).toBe(4);
    expect(await restoredTexts(projectRoot, "s1")).toEqual([
      "旧一问",
      "旧一答",
      "新二问",
      "新二答",
    ]);
  });

  it("单分支：branch 后恢复走新路径，弃用段剪除；branch_moved 不入链", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    // r2 落在主路径上，随后被弃用
    await appendCommittedRound(projectRoot, "s1", "r2", "第二问（弃）", "第二答（弃）");
    const events = await readTranscriptEvents(projectRoot, "s1");
    const commit1Seq = events.find(
      (event) => event.type === "request_committed" && event.requestId === "r1",
    )!.seq;

    const result = await branchBookSession(projectRoot, "s1", commit1Seq);
    expect(result).toEqual({ head: commit1Seq, branchCount: 1 });

    await appendCommittedRound(projectRoot, "s1", "r2b", "第二问", "第二答");

    const after = await readTranscriptEvents(projectRoot, "s1");
    const chain = activeChainEvents(after);
    // branch_moved 是元事件不入链；弃用 r2 整体剪除
    expect(chain.every((event) => event.type !== "branch_moved")).toBe(true);
    const texts = await restoredTexts(projectRoot, "s1");
    expect(texts).toEqual(["第一问", "第一答", "第二问", "第二答"]);
    expect(texts.join()).not.toContain("弃");

    // 重启 replay：head 从事件流重建（fresh 读，无内存态）
    expect(transcriptHead(after)).toBe(after.at(-1)?.seq ?? null);
  });

  it("多分支：从新分支再分叉（整段弃用重写），恢复只看最末路径", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    const events = await readTranscriptEvents(projectRoot, "s1");
    const commit1Seq = events.find(
      (event) => event.type === "request_committed" && event.requestId === "r1",
    )!.seq;

    // 第一次分支：回 r1 提交点，写 r1a（占位稿）
    await branchBookSession(projectRoot, "s1", commit1Seq);
    await appendCommittedRound(projectRoot, "s1", "r1a", "（占位）", "（占位答）");
    const after = await readTranscriptEvents(projectRoot, "s1");
    const r1aStartedSeq = after.find(
      (event) => event.type === "request_started" && event.requestId === "r1a",
    )!.seq;

    // 第二次分支：回 r1a 起点（整段弃用 r1a），重写 r1b
    const second = await branchBookSession(projectRoot, "s1", r1aStartedSeq);
    expect(second).toEqual({ head: r1aStartedSeq, branchCount: 2 });
    await appendCommittedRound(projectRoot, "s1", "r1b", "改写一问", "改写一答");

    const texts = await restoredTexts(projectRoot, "s1");
    // r1 完整在新路径上（r1a 起点为分支点），r1a 整体弃用，r1b 接续
    expect(texts).toEqual(["第一问", "第一答", "改写一问", "改写一答"]);
  });

  it("分支到未提交请求中点：该请求消息按设计剪除（未 committed 的 request 不回放）", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    const events = await readTranscriptEvents(projectRoot, "s1");
    const user1Seq = events.find(
      (event) => event.type === "message" && event.role === "user",
    )!.seq;

    // 分支到 r1 的 user 消息：r1 的 request_committed 不在新路径上，
    // 该请求视为未提交——user 消息虽在链上但不回放（设计语义）。
    await branchBookSession(projectRoot, "s1", user1Seq);
    await appendCommittedRound(projectRoot, "s1", "r1b", "改写一问", "改写一答");

    expect(await restoredTexts(projectRoot, "s1")).toEqual(["改写一问", "改写一答"]);
  });

  it("resetLeaf（toSeq=null）：head 置空，新写入开独立链根，恢复只见新链", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "旧问", "旧答");
    const result = await branchBookSession(projectRoot, "s1", null);
    expect(result).toEqual({ head: null, branchCount: 1 });
    expect(await restoredTexts(projectRoot, "s1")).toEqual([]);

    await appendCommittedRound(projectRoot, "s1", "r2", "新链问", "新链答");
    const events = await readTranscriptEvents(projectRoot, "s1");
    const started2 = events.find(
      (event) => event.type === "request_started" && event.requestId === "r2",
    )!;
    expect(started2.parentSeq).toBeNull();
    expect(await restoredTexts(projectRoot, "s1")).toEqual(["新链问", "新链答"]);
  });

  it("branch 后重启 replay：head 从落盘事件流重建（跨进程持久）", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    const events = await readTranscriptEvents(projectRoot, "s1");
    const commitSeq = events[events.length - 1].seq;
    await branchBookSession(projectRoot, "s1", commitSeq);

    // 模拟重启：全新读取（无内存 head 状态）
    const fresh = await readTranscriptEvents(projectRoot, "s1");
    expect(transcriptHead(fresh)).toBe(commitSeq);
    expect(fresh.filter((event) => event.type === "branch_moved")).toHaveLength(1);
  });

  it("branch 目标 seq 不存在 → BranchTargetNotFoundError；空会话 → null", async () => {
    await expect(branchBookSession(projectRoot, "ghost", 1)).resolves.toBeNull();

    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    await expect(branchBookSession(projectRoot, "s1", 99)).rejects.toBeInstanceOf(
      BranchTargetNotFoundError,
    );
  });

  it("压缩边界随分支互不污染：弃用路径上的 compaction 不影响新分支恢复窗口", async () => {
    const { userUuid } = await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    await appendCommittedRound(projectRoot, "s1", "r2", "第二问", "第二答");
    // 主路径上压缩：窗口从 r2 的 user 消息起
    await appendTranscriptEvents(projectRoot, "s1", ({ nextSeq }) => [
      {
        type: "compaction",
        version: 1,
        sessionId: "s1",
        requestId: "compact-1",
        seq: nextSeq,
        timestamp: Date.now(),
        summary: "此前对话摘要",
        firstKeptUuid: "u-r2-user",
        tokensBefore: 100,
        trigger: "threshold",
      },
    ]);
    // 压缩生效：恢复窗口只有 r2
    expect(await restoredTexts(projectRoot, "s1")).toEqual(["第二问", "第二答"]);

    // 分支回 r1 之后：compaction 在弃用路径上，新分支恢复全量
    const events = await readTranscriptEvents(projectRoot, "s1");
    const commit1Seq = events.find(
      (event) => event.type === "request_committed" && event.requestId === "r1",
    )!.seq;
    expect(commit1Seq).toBeDefined();
    void userUuid;
    await branchBookSession(projectRoot, "s1", commit1Seq);
    await appendCommittedRound(projectRoot, "s1", "r1b", "分支新问", "分支新答");

    expect(await restoredTexts(projectRoot, "s1")).toEqual([
      "第一问",
      "第一答",
      "分支新问",
      "分支新答",
    ]);
  });

  it("derive 透出 head/branchCount；对话消息取 active 路径", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    await appendCommittedRound(projectRoot, "s1", "r2", "第二问", "第二答");
    const events = await readTranscriptEvents(projectRoot, "s1");
    const commit1Seq = events.find(
      (event) => event.type === "request_committed" && event.requestId === "r1",
    )!.seq;
    await branchBookSession(projectRoot, "s1", commit1Seq);
    await appendCommittedRound(projectRoot, "s1", "r2b", "第二问", "第二答");

    const session = await deriveBookSessionFromTranscript(projectRoot, "s1");
    expect(session).not.toBeNull();
    expect(session!.branchCount).toBe(1);
    const lastSeq = (await readTranscriptEvents(projectRoot, "s1")).at(-1)!.seq;
    expect(session!.head).toBe(lastSeq);
    // active 路径：r1 + r2b（弃用 r2 不在 derive 消息里）
    const contents = session!.messages.map((message) =>
      typeof message.content === "string"
        ? message.content
        : JSON.stringify(message.content),
    );
    expect(contents.join()).toContain("第一问");
    expect(contents.join()).toContain("第二问");
    expect(contents.filter((text) => text.includes("第二问"))).toHaveLength(1);
  });

  it("schema golden：branch_moved 行解析 + fromSeq/toSeq 可 null", () => {
    const parsed = TranscriptEventSchema.parse({
      type: "branch_moved",
      version: 1,
      sessionId: "s1",
      seq: 9,
      timestamp: 9,
      parentSeq: 8,
      fromSeq: 8,
      toSeq: null,
    });
    expect(parsed.type).toBe("branch_moved");
    expect(TranscriptEventSchema.safeParse({
      type: "branch_moved",
      version: 1,
      sessionId: "s1",
      seq: 9,
      timestamp: 9,
      fromSeq: "x",
      toSeq: null,
    }).success).toBe(false);
  });

  it("读回的 transcript 文件：新写入行恒带 parentSeq 键（含 null）", async () => {
    await appendCommittedRound(projectRoot, "s1", "r1", "第一问", "第一答");
    const raw = await readFile(transcriptPath(projectRoot, "s1"), "utf-8");
    for (const line of raw.split(/\r?\n/).filter((line) => line.trim())) {
      expect(line).toContain('"parentSeq"');
    }
  });
});
