import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { transcriptPath } from "@actalk/inkos-core";

/**
 * R36 会话树化（636-639 号；692 号随 2.0 合并重施）：分支点读面 + 分支切换
 * 路由端到端——播种 transcript（两轮 request_started/committed）后：
 * GET branches（committed 全集 + preview + onActiveChain）→ POST branch（head
 * 移动 + branchCount 增长 + 弃用链标记翻转）→ 非法目标 400 → 缺会话 404。
 * 与 engine-rs session_branch 路由同水位对偶。
 */
describe("session branch routes (R36, 692 重施)", () => {
  let root: string;
  const SESSION = "sess-branch-test";

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "session-branch-"));
    await mkdir(join(root, ".inkos", "sessions"), { recursive: true });
    const base = { version: 1 as const, sessionId: SESSION };
    const events = [
      { ...base, type: "session_created", seq: 0, timestamp: 1_000, bookId: null, title: null, createdAt: 1_000, updatedAt: 1_000 },
      { ...base, type: "request_started", seq: 1, timestamp: 1_001, requestId: "r1", input: "第一轮提问" },
      { ...base, type: "request_committed", seq: 2, timestamp: 1_002, requestId: "r1" },
      { ...base, type: "request_started", seq: 3, timestamp: 1_003, requestId: "r2", input: "第二轮提问" },
      { ...base, type: "request_committed", seq: 4, timestamp: 1_004, requestId: "r2" },
    ];
    await writeFile(
      transcriptPath(root, SESSION),
      events.map((e) => JSON.stringify(e)).join("\n") + "\n",
      "utf-8",
    );
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("derives branch points, moves head on branch, rejects invalid targets", async () => {
    const app = createStudioServer({} as never, root);

    // 分支点读面：两轮 committed 全集，初始都在当前链上
    const list = await app.request(`/api/v1/sessions/${SESSION}/branches`);
    expect(list.status).toBe(200);
    const listBody = await list.json() as {
      sessionId: string;
      head: number | null;
      branchCount: number;
      points: ReadonlyArray<{ seq: number; requestId: string; preview: string; onActiveChain: boolean }>;
    };
    expect(listBody.sessionId).toBe(SESSION);
    expect(listBody.head).toBe(4);
    expect(listBody.branchCount).toBe(0);
    expect(listBody.points.map((p) => p.seq)).toEqual([2, 4]);
    expect(listBody.points[0]?.preview).toBe("第一轮提问");
    expect(listBody.points.every((p) => p.onActiveChain)).toBe(true);

    // branch 回第一轮：head 移到 seq=2，branchCount=1
    const branch = await app.request(`/api/v1/sessions/${SESSION}/branch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ toSeq: 2 }),
    });
    expect(branch.status).toBe(200);
    const branchBody = await branch.json() as { ok: boolean; head: number | null; branchCount: number };
    expect(branchBody.ok).toBe(true);
    expect(branchBody.head).toBe(2); // replay 语义：branch 后 head=toSeq（非事件自身 seq）
    expect(branchBody.branchCount).toBe(1);

    // 弃用链标记翻转：seq=4（第二轮 committed）不再在当前链上
    const after = await app.request(`/api/v1/sessions/${SESSION}/branches`);
    const afterBody = await after.json() as { points: ReadonlyArray<{ seq: number; onActiveChain: boolean }> };
    expect(afterBody.points.find((p) => p.seq === 2)?.onActiveChain).toBe(true);
    expect(afterBody.points.find((p) => p.seq === 4)?.onActiveChain).toBe(false);

    // 非法目标（不存在的 seq）→ 400
    const invalid = await app.request(`/api/v1/sessions/${SESSION}/branch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ toSeq: 999 }),
    });
    expect(invalid.status).toBe(400);
    const invalidBody = await invalid.json() as { error: { code?: string } | string };
    expect(JSON.stringify(invalidBody.error)).toContain("INVALID_BRANCH_TARGET");

    // 非法 toSeq 形态 → 400
    const malformed = await app.request(`/api/v1/sessions/${SESSION}/branch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ toSeq: -1 }),
    });
    expect(malformed.status).toBe(400);

    // 缺会话 → 404
    const missing = await app.request("/api/v1/sessions/sess-missing/branches");
    expect(missing.status).toBe(404);
  });

  // 1663 号：derive/list 透出 head/branchCount（636 号契约面；692 号重施遗漏，
  // Node 腿会话探针活体抓出）——Rust BookSession/BookSessionSummary 同形。
  it("exposes head/branchCount on the session derive and list projections", async () => {
    const app = createStudioServer({} as never, root);
    const treeOf = (session: { head: unknown; branchCount: unknown }) => [session.head, session.branchCount];

    // 分支前：head=链尾 seq=4，未分支
    const before = await app.request(`/api/v1/sessions/${SESSION}`);
    expect(before.status).toBe(200);
    expect(treeOf(((await before.json()) as { session: { head: unknown; branchCount: unknown } }).session)).toEqual([4, 0]);
    const listedBefore = await app.request("/api/v1/sessions?bookId=null");
    const summariesBefore = (await listedBefore.json()) as { sessions: Array<{ sessionId: string; head: unknown; branchCount: unknown }> };
    expect(treeOf(summariesBefore.sessions.find((s) => s.sessionId === SESSION)!)).toEqual([4, 0]);

    // 分支后：derive 与 list 同步移动（与 branch 响应/branches 读面单源）
    const branch = await app.request(`/api/v1/sessions/${SESSION}/branch`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ toSeq: 2 }),
    });
    expect(branch.status).toBe(200);
    const after = await app.request(`/api/v1/sessions/${SESSION}`);
    const derived = ((await after.json()) as { session: Record<string, unknown> }).session;
    expect(treeOf(derived as { head: unknown; branchCount: unknown })).toEqual([2, 1]);
    const listedAfter = await app.request("/api/v1/sessions?bookId=null");
    const summariesAfter = (await listedAfter.json()) as { sessions: Array<{ sessionId: string; head: unknown; branchCount: unknown }> };
    expect(treeOf(summariesAfter.sessions.find((s) => s.sessionId === SESSION)!)).toEqual([2, 1]);

    // 2.0 会话形状字段不受加法影响
    expect(derived.sessionId).toBe(SESSION);
    expect(Array.isArray(derived.messages)).toBe(true);
  });
});
