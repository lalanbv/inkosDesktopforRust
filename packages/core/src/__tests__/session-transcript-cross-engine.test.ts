//! 876 号：跨引擎 transcript 兼容金标准（TS 侧）。三条金标准行与 Rust 侧
//! `engine-rs/src/interaction/session_transcript.rs` 的 `mod tests` **逐字共用**——
//! 双端恒等锁：任一端改 schema 而另一端不跟，必有一端红。
//!
//! 背景：上游 2.0（b399a8d3）把消息事件 `legacyDisplay` 改名 `display` 未做迁移，
//! 876 前 Rust 引擎与 2.0 前本端写出的旧名行被严格 schema 整份拒读——桌面默认
//! Rust 引擎、故障回退 Node 时同一项目的会话由两个引擎交替读写。
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  normalizeLegacyTranscriptLine,
  readTranscriptEvents,
  transcriptPath,
} from "../interaction/session-transcript.js";

const GOLDEN_LEGACY_DISPLAY_LINE = `{"type":"message","version":1,"sessionId":"s876","seq":3,"timestamp":1730000000000,"parentSeq":2,"requestId":"req-1","uuid":"u-3","parentUuid":null,"role":"assistant","legacyDisplay":{"toolExecutions":[{"id":"exec-1","tool":"draft_structure","label":"draft_structure","status":"completed"}]},"message":{"role":"assistant","content":[{"type":"text","text":"已生成结构"}]}}`;
const GOLDEN_TS20_MESSAGE_LINE = `{"type":"message","version":1,"sessionId":"s876","seq":4,"timestamp":1730000000001,"parentSeq":3,"requestId":"req-1","uuid":"u-4","parentUuid":"u-3","role":"assistant","visibility":"model","display":{"completion":{"status":"answered","message":"完成"},"userInput":{"text":"写大纲","language":"zh","attachments":[]},"thinking":"先列人物","toolExecutions":[{"id":"exec-2","tool":"write_outline","status":"completed"}]},"message":{"role":"assistant","content":[]}}`;
const GOLDEN_REQUEST_DELIVERY_LINE = `{"type":"request_delivery","version":1,"sessionId":"s876","seq":5,"timestamp":1730000000002,"parentSeq":4,"requestId":"req-1","state":{"version":1,"authorRequest":"写大纲","declared":true,"steps":[],"receipts":[]}}`;
const SESSION_CREATED_LINE = `{"type":"session_created","version":1,"sessionId":"s876","seq":1,"timestamp":1,"parentSeq":null,"bookId":null,"title":null,"createdAt":1,"updatedAt":1}`;
const REQUEST_STARTED_LINE = `{"type":"request_started","version":1,"sessionId":"s876","seq":2,"timestamp":2,"parentSeq":1,"requestId":"req-1","input":"写大纲"}`;

describe("跨引擎 transcript 兼容（876）", () => {
  let root: string;

  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "inkos-876-"));
    await mkdir(join(root, ".inkos", "sessions"), { recursive: true });
  });

  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("混合旧名/2.0/交付账本的会话文件整份可读（修复前旧名行致整份拒读）", async () => {
    await writeFile(
      transcriptPath(root, "s876"),
      [SESSION_CREATED_LINE, REQUEST_STARTED_LINE, GOLDEN_LEGACY_DISPLAY_LINE, GOLDEN_TS20_MESSAGE_LINE, GOLDEN_REQUEST_DELIVERY_LINE].join("\n") + "\n",
      "utf-8",
    );
    const events = await readTranscriptEvents(root, "s876");
    expect(events.map((event) => event.type)).toEqual([
      "session_created", "request_started", "message", "message", "request_delivery",
    ]);
    const legacy = events[2];
    expect(legacy.type === "message" && legacy.display?.toolExecutions).toHaveLength(1);
    expect(legacy).not.toHaveProperty("legacyDisplay");
    const ts20 = events[3];
    expect(ts20.type === "message" && ts20.visibility).toBe("model");
    expect(ts20.type === "message" && ts20.display?.thinking).toBe("先列人物");
  });

  it("normalizeLegacyTranscriptLine：新名优先、空旧名丢弃、非 message 原样", () => {
    const both = normalizeLegacyTranscriptLine({ type: "message", display: { thinking: "新" }, legacyDisplay: { thinking: "旧" } });
    expect(both).toEqual({ type: "message", display: { thinking: "新" } });
    expect(normalizeLegacyTranscriptLine({ type: "message", legacyDisplay: null })).toEqual({ type: "message" });
    const other = { type: "request_started", legacyDisplay: { thinking: "x" } };
    expect(normalizeLegacyTranscriptLine(other)).toBe(other);
    expect(normalizeLegacyTranscriptLine("not-an-object")).toBe("not-an-object");
  });
});
