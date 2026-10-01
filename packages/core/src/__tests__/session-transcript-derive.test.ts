import { describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { readFileSync } from "node:fs";
import { readTranscriptEvents } from "../interaction/session-transcript.js";
import {
  committedMessageEvents,
  deriveBookSessionFromTranscript,
  restoreCommittedDialogueScan,
} from "../interaction/session-transcript-restore.js";
import type { SessionKind } from "../interaction/session.js";

/**
 * R43/565 号会话 transcript derive/restore 双端共享 golden 回放：与 Rust 侧
 * `engine-rs/tests/golden_session_transcript_derive.rs` 消费同一份向量——
 * committed 过滤（未提交轮整体排除=中断尾部确定性修复）、kind 过滤（仅模型
 * 面）、工具轮折叠卡与 legacy user 保留、空文本 assistant 的面间不对称，
 * 码点级锁死。改向量必须双端同批。
 */

const VECTORS: {
  version: number;
  cases: Array<{
    name: string;
    deriveSessionKind: string | null;
    events: Array<Record<string, unknown>>;
    expected: {
      committedUuids: string[];
      scanMessages: Array<{ role: string; content: string }>;
      deriveRoles: string[];
      deriveToolCardCount?: number;
    };
  }>;
} = JSON.parse(
  readFileSync(
    new URL("./golden/session-transcript-derive-vectors.json", import.meta.url),
    "utf8",
  ),
);

/** scanMessages.content 口径：文本块拍平（Rust 侧 flatten_content 同构）。 */
function flattenContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter(
        (block): block is { type: "text"; text: string } =>
          !!block && typeof block === "object" && (block as { type?: string }).type === "text",
      )
      .map((block) => block.text)
      .join("");
  }
  return "";
}

describe("session transcript derive shared golden vectors (R43 / 565)", () => {
  it("loads the shared vector file", () => {
    expect(VECTORS.version).toBe(1);
    expect(VECTORS.cases.length).toBeGreaterThan(0);
  });

  for (const testCase of VECTORS.cases) {
    it(`case: ${testCase.name}`, async () => {
      const projectRoot = await mkdtemp(join(tmpdir(), "inkos-st-derive-"));
      try {
        const sessionsDir = join(projectRoot, ".inkos", "sessions");
        await mkdir(sessionsDir, { recursive: true });
        // wire 形态逐行落盘：readTranscriptEvents 的 zod 解析必须全收。
        const raw = testCase.events.map((event) => JSON.stringify(event)).join("\n") + "\n";
        await writeFile(join(sessionsDir, "s.jsonl"), raw, "utf-8");

        const events = await readTranscriptEvents(projectRoot, "s");
        expect(events.length, "解析器必须全收向量事件").toBe(testCase.events.length);

        const kind = testCase.deriveSessionKind;
        const committed = committedMessageEvents(
          events,
          (kind as SessionKind | undefined) ?? undefined,
        );
        expect(committed.map((event) => event.uuid)).toEqual(testCase.expected.committedUuids);

        const scan = restoreCommittedDialogueScan(
          events,
          (kind as SessionKind | undefined) ?? undefined,
        );
        expect(
          scan.messages.map((message) => {
            const raw = message as unknown as { role?: unknown; content?: unknown };
            return { role: String(raw.role), content: flattenContent(raw.content) };
          }),
        ).toEqual(testCase.expected.scanMessages);

        const session = await deriveBookSessionFromTranscript(projectRoot, "s");
        expect(session).not.toBeNull();
        expect(session!.messages.map((message) => message.role)).toEqual(
          testCase.expected.deriveRoles,
        );
        if (testCase.expected.deriveToolCardCount !== undefined) {
          const cardCount = session!.messages.filter((message) =>
            Array.isArray(message.toolExecutions),
          ).length;
          expect(cardCount).toBe(testCase.expected.deriveToolCardCount);
        }
      } finally {
        await rm(projectRoot, { recursive: true, force: true });
      }
    });
  }
});
