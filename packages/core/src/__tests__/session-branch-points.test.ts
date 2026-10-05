import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  appendTranscriptEvents,
  transcriptPath,
} from "../interaction/session-transcript.js";
import {
  branchBookSession,
  deriveSessionBranchPoints,
} from "../interaction/book-session-store.js";
import type { TranscriptEvent } from "../interaction/session-transcript-schema.js";

let root: string;

beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), "inkos-branch-points-"));
});

afterEach(async () => {
  await rm(root, { recursive: true, force: true });
});

/** 一轮已提交对话：request_started + user + assistant + request_committed。 */
async function appendCommittedRound(
  projectRoot: string,
  sessionId: string,
  requestId: string,
  userText: string,
): Promise<void> {
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
          content: [{ type: "text", text: `回复：${userText}` }],
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
}

async function appendRawLines(projectRoot: string, sessionId: string, lines: string[]): Promise<void> {
  const { mkdir, writeFile, appendFile } = await import("node:fs/promises");
  await mkdir(join(projectRoot, ".inkos", "sessions"), { recursive: true });
  const payload = lines.join("\n") + "\n";
  try {
    await appendFile(transcriptPath(projectRoot, sessionId), payload);
  } catch {
    await writeFile(transcriptPath(projectRoot, sessionId), payload);
  }
}

describe("deriveSessionBranchPoints（638 号）", () => {
  it("线性两轮：两点全在链上，seq=各轮 request_committed，preview=input", async () => {
    const sessionId = "1783000000000-lin";
    await appendCommittedRound(root, sessionId, "r1", "第一个问题");
    await appendCommittedRound(root, sessionId, "r2", "第二个问题");

    const result = await deriveSessionBranchPoints(root, sessionId);
    expect(result).not.toBeNull();
    expect(result!.sessionId).toBe(sessionId);
    expect(result!.branchCount).toBe(0);
    expect(result!.points).toHaveLength(2);
    expect(result!.points[0]!.preview).toBe("第一个问题");
    expect(result!.points[1]!.preview).toBe("第二个问题");
    expect(result!.points.every((point) => point.onActiveChain)).toBe(true);
    // head = 文件最后一个事件 seq（第二轮 committed）
    const events = await import("../interaction/session-transcript.js").then((m) => m.readTranscriptEvents(root, sessionId));
    const lastSeq = events[events.length - 1]!.seq;
    expect(result!.points[1]!.seq).toBe(lastSeq);
    expect(result!.head).toBe(lastSeq);
  });

  it("分支后：旧链提交点 onActiveChain=false，新链提交点 true，branchCount=1", async () => {
    const sessionId = "1783000001000-br";
    await appendCommittedRound(root, sessionId, "r1", "第一轮");
    await appendCommittedRound(root, sessionId, "r2", "第二轮（将弃用）");
    const before = await deriveSessionBranchPoints(root, sessionId);
    const firstCommitSeq = before!.points[0]!.seq;

    await branchBookSession(root, sessionId, firstCommitSeq);
    await appendCommittedRound(root, sessionId, "r2b", "第二轮重写");

    const after = await deriveSessionBranchPoints(root, sessionId);
    expect(after!.branchCount).toBe(1);
    expect(after!.points).toHaveLength(3);
    const bySeq = new Map(after!.points.map((point) => [point.seq, point]));
    expect(bySeq.get(firstCommitSeq)!.onActiveChain).toBe(true);
    const abandoned = after!.points.find((point) => !point.onActiveChain)!;
    expect(abandoned.preview).toBe("第二轮（将弃用）");
    expect(after!.points.find((point) => point.preview === "第二轮重写")!.onActiveChain).toBe(true);
  });

  it("失败轮（request_failed 无 committed）不产生分支点", async () => {
    const sessionId = "1783000002000-fail";
    await appendCommittedRound(root, sessionId, "r1", "成功轮");
    await appendTranscriptEvents(root, sessionId, ({ nextSeq }) => [{
      type: "request_failed",
      version: 1,
      sessionId,
      requestId: "r-failed",
      seq: nextSeq,
      timestamp: Date.now(),
      error: "boom",
    }]);
    const result = await deriveSessionBranchPoints(root, sessionId);
    expect(result!.points).toHaveLength(1);
    expect(result!.points[0]!.preview).toBe("成功轮");
  });

  it("无 request_started 的 committed（手工构造）preview 空串兜底；会话不存在返回 null", async () => {
    const sessionId = "1783000003000-raw";
    const now = Date.now();
    await appendRawLines(root, sessionId, [
      JSON.stringify({
        type: "request_committed", version: 1, sessionId, requestId: "orphan",
        seq: 0, timestamp: now, parentSeq: null,
      }),
    ]);
    const result = await deriveSessionBranchPoints(root, sessionId);
    expect(result).not.toBeNull();
    expect(result!.points).toHaveLength(1);
    expect(result!.points[0]!.preview).toBe("");
    expect(result!.points[0]!.onActiveChain).toBe(true);

    expect(await deriveSessionBranchPoints(root, "no-such-session")).toBeNull();
  });

  it("resetLeaf（branch toSeq=null）后链空：全部点 onActiveChain=false，head=null", async () => {
    const sessionId = "1783000004000-reset";
    await appendCommittedRound(root, sessionId, "r1", "第一轮");
    await branchBookSession(root, sessionId, null);
    const result = await deriveSessionBranchPoints(root, sessionId);
    expect(result!.head).toBeNull();
    expect(result!.branchCount).toBe(1);
    expect(result!.points).toHaveLength(1);
    expect(result!.points[0]!.onActiveChain).toBe(false);
  });
});
