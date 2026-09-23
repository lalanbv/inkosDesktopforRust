import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { STUDIO_SSE_EVENTS, collectNewSSEMessages } from "./use-sse";
import type { SSEMessage } from "./use-sse";

// R31 golden（549 号；542 号施工图 §5 第 3 组）：SSE 事件名集合精确快照
// （492 号事件面）——前端合法接收名单全集锁定，新增/删除/改名必须显式更新
// golden/studio-sse-events.json 并同步 server broadcast 面。
const goldenSSE = JSON.parse(
  readFileSync(join(fileURLToPath(new URL(".", import.meta.url)), "..", "__tests__", "golden", "studio-sse-events.json"), "utf8"),
) as { events: string[] };

describe("STUDIO_SSE_EVENTS", () => {
  it("matches the golden event-name snapshot exactly (R31 / 492 号面)", () => {
    expect([...STUDIO_SSE_EVENTS]).toEqual(goldenSSE.events);
  });

  it("covers the server lifecycle events that drive the UI", () => {
    expect(STUDIO_SSE_EVENTS).toEqual(expect.arrayContaining([
      "book:creating",
      "book:created",
      "book:deleted",
      "book:error",
      "write:start",
      "write:complete",
      "write:error",
      "draft:start",
      "draft:complete",
      "draft:error",
      "daemon:started",
      "daemon:stopped",
      "daemon:error",
      "audit:start",
      "audit:complete",
      "audit:error",
      "revise:start",
      "revise:complete",
      "revise:error",
      "rewrite:start",
      "rewrite:complete",
      "rewrite:error",
      "agent:start",
      "agent:complete",
      "agent:error",
      "import:start",
      "import:complete",
      "import:error",
      "fanfic:start",
      "fanfic:complete",
      "fanfic:error",
      "fanfic:refresh:start",
      "fanfic:refresh:complete",
      "fanfic:refresh:error",
      "style:start",
      "style:complete",
      "style:error",
      "radar:start",
      "radar:complete",
      "radar:error",
      "log",
      "llm:progress",
      "ping",
    ]));
  });
});

function msg(seq: number, event = "log"): SSEMessage {
  return { event, data: null, timestamp: 1000 + seq, seq };
}

describe("collectNewSSEMessages", () => {
  it("returns every message after the cursor, not just the last one", () => {
    const messages = [msg(1), msg(2, "book:created"), msg(3, "session:title"), msg(4)];
    const { fresh, nextCursor } = collectNewSSEMessages(messages, 1);
    expect(fresh.map((message) => message.seq)).toEqual([2, 3, 4]);
    expect(nextCursor).toBe(4);
  });

  it("skips the backlog on first subscription and only sets the cursor", () => {
    const messages = [msg(1), msg(2)];
    const { fresh, nextCursor } = collectNewSSEMessages(messages, null);
    expect(fresh).toEqual([]);
    expect(nextCursor).toBe(2);
  });

  it("keeps a null cursor while the buffer is empty", () => {
    const { fresh, nextCursor } = collectNewSSEMessages([], null);
    expect(fresh).toEqual([]);
    expect(nextCursor).toBeNull();
  });

  it("survives buffer trimming as long as retained messages are after the cursor", () => {
    const messages = [msg(50), msg(51), msg(52)];
    const { fresh, nextCursor } = collectNewSSEMessages(messages, 49);
    expect(fresh.map((message) => message.seq)).toEqual([50, 51, 52]);
    expect(nextCursor).toBe(52);
  });
});
