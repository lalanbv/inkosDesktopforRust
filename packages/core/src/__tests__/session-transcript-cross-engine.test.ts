//! 876 号：跨引擎 transcript 兼容金标准（TS 侧）。金标准行与 Rust 侧
//! `engine-rs/src/interaction/session_transcript.rs` 的 `mod tests` **逐字共用**——
//! 双端恒等锁：任一端改 schema 而另一端不跟，必有一端红。
//!
//! 背景：上游 2.0（b399a8d3）把消息事件 `legacyDisplay` 改名 `display` 未做迁移，
//! 876 前 Rust 引擎与 2.0 前本端写出的旧名行被严格 schema 整份拒读——桌面默认
//! Rust 引擎、故障回退 Node 时同一项目的会话由两个引擎交替读写。
//!
//! 1666 号：读面投影族对偶（`TS20_FINISH_TURN_LINES` 及其后的行集同 Rust
//! 逐字共用）。Rust 腿此前不消费 `display.completion.message`——2.0 收尾协议
//! 写的每个回合在产品引擎里读出来都没有助手回复；同族还差 `display.userInput`、
//! `能力域__动作` 剥离、沉浸场景独立卡、协议工具不记卡、读面二次截断。本侧
//! 断言钉的是参考行为（Rust 逐项对齐于此）：上游改动投影行为时这里先红。
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  normalizeLegacyTranscriptLine,
  readTranscriptEvents,
  transcriptPath,
} from "../interaction/session-transcript.js";
import { deriveBookSessionFromTranscript } from "../interaction/session-transcript-restore.js";

const GOLDEN_LEGACY_DISPLAY_LINE = `{"type":"message","version":1,"sessionId":"s876","seq":3,"timestamp":1730000000000,"parentSeq":2,"requestId":"req-1","uuid":"u-3","parentUuid":null,"role":"assistant","legacyDisplay":{"toolExecutions":[{"id":"exec-1","tool":"draft_structure","label":"draft_structure","status":"completed"}]},"message":{"role":"assistant","content":[{"type":"text","text":"已生成结构"}]}}`;
const GOLDEN_TS20_MESSAGE_LINE = `{"type":"message","version":1,"sessionId":"s876","seq":4,"timestamp":1730000000001,"parentSeq":3,"requestId":"req-1","uuid":"u-4","parentUuid":"u-3","role":"assistant","visibility":"model","display":{"completion":{"status":"answered","message":"完成"},"userInput":{"text":"写大纲","language":"zh","attachments":[]},"thinking":"先列人物","toolExecutions":[{"id":"exec-2","tool":"write_outline","status":"completed"}]},"message":{"role":"assistant","content":[]}}`;
const GOLDEN_REQUEST_DELIVERY_LINE = `{"type":"request_delivery","version":1,"sessionId":"s876","seq":5,"timestamp":1730000000002,"parentSeq":4,"requestId":"req-1","state":{"version":1,"authorRequest":"写大纲","declared":true,"steps":[],"receipts":[]}}`;
const SESSION_CREATED_LINE = `{"type":"session_created","version":1,"sessionId":"s876","seq":1,"timestamp":1,"parentSeq":null,"bookId":null,"title":null,"createdAt":1,"updatedAt":1}`;
const REQUEST_STARTED_LINE = `{"type":"request_started","version":1,"sessionId":"s876","seq":2,"timestamp":2,"parentSeq":1,"requestId":"req-1","input":"写大纲"}`;

//! 上游 2.0 TS 腿真实写出的一个完整回合（1666 号活体 transcript 逐字摘录，
//! 仅 id/时间戳定值）：`finish_turn` 收尾协议——工具调用与结果都是
//! `visibility:"model"`，作者可见答复在收尾消息的 `display.completion.message`，
//! 正文 `content:[]`。
const TS20_FINISH_TURN_LINES = [
  `{"type":"message","version":1,"sessionId":"s20","seq":5,"timestamp":105,"parentSeq":4,"requestId":"req-20","uuid":"u-5","parentUuid":null,"role":"user","piTurnIndex":1,"message":{"role":"user","content":[{"type":"text","text":"打个招呼"}],"timestamp":105}}`,
  `{"type":"message","version":1,"sessionId":"s20","seq":6,"timestamp":106,"parentSeq":5,"requestId":"req-20","uuid":"u-6","parentUuid":"u-5","role":"assistant","visibility":"model","piTurnIndex":1,"toolCallId":"call-finish","message":{"role":"assistant","content":[{"type":"toolCall","id":"call-finish","name":"finish_turn","arguments":{"status":"answered","message":"OK"}}],"stopReason":"toolUse","timestamp":106}}`,
  `{"type":"message","version":1,"sessionId":"s20","seq":7,"timestamp":107,"parentSeq":6,"requestId":"req-20","uuid":"u-7","parentUuid":"u-6","role":"toolResult","visibility":"model","piTurnIndex":1,"toolCallId":"call-finish","sourceToolAssistantUuid":"u-6","message":{"role":"toolResult","toolCallId":"call-finish","toolName":"finish_turn","content":[{"type":"text","text":"OK"}],"details":{"kind":"turn_completion","status":"answered","message":"OK"},"isError":false,"timestamp":107}}`,
  `{"type":"message","version":1,"sessionId":"s20","seq":8,"timestamp":108,"parentSeq":7,"requestId":"req-20","uuid":"u-8","parentUuid":"u-7","role":"assistant","piTurnIndex":2,"display":{"completion":{"status":"answered","message":"OK"}},"message":{"role":"assistant","content":[],"stopReason":"stop","timestamp":108}}`,
  `{"type":"request_committed","version":1,"sessionId":"s20","seq":9,"timestamp":109,"parentSeq":8,"requestId":"req-20"}`,
];

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

  // ── 1666 号：读面投影族对偶（行集与期望值同 Rust `mod tests`）──

  const derive = async (lines: ReadonlyArray<string>) => {
    await writeFile(transcriptPath(root, "s20"), `${lines.join("\n")}\n`, "utf-8");
    return deriveBookSessionFromTranscript(root, "s20");
  };
  /** fork 产品引擎写出的一个带工具调用的已提交回合（engine-rs `commit_chat_turn` 形态）。 */
  const rustTurn = (resultText: string, isError: boolean) => [
    `{"type":"session_created","version":1,"sessionId":"s20","seq":1,"timestamp":1,"parentSeq":null,"bookId":null,"title":null,"createdAt":1,"updatedAt":1}`,
    `{"type":"request_started","version":1,"sessionId":"s20","seq":2,"timestamp":2,"parentSeq":1,"requestId":"req-1","sessionKind":"chat","input":"读文件"}`,
    `{"type":"message","version":1,"sessionId":"s20","seq":3,"timestamp":3,"parentSeq":2,"requestId":"req-1","uuid":"u-3","parentUuid":null,"role":"user","message":{"role":"user","content":"读文件","timestamp":3}}`,
    `{"type":"message","version":1,"sessionId":"s20","seq":4,"timestamp":4,"parentSeq":3,"requestId":"req-1","uuid":"u-4","parentUuid":"u-3","role":"assistant","message":{"role":"assistant","content":[{"type":"toolCall","id":"call-1","name":"read","arguments":{"path":"a.md"}}]}}`,
    `{"type":"message","version":1,"sessionId":"s20","seq":5,"timestamp":5,"parentSeq":4,"requestId":"req-1","uuid":"u-5","parentUuid":"u-4","role":"toolResult","toolCallId":"call-1","sourceToolAssistantUuid":"u-4","message":${JSON.stringify({
      role: "toolResult", toolCallId: "call-1", content: [{ type: "text", text: resultText }], isError, timestamp: 5,
    })}}`,
    `{"type":"message","version":1,"sessionId":"s20","seq":6,"timestamp":6,"parentSeq":5,"requestId":"req-1","uuid":"u-6","parentUuid":null,"role":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"读完了"}]}}`,
    `{"type":"request_committed","version":1,"sessionId":"s20","seq":7,"timestamp":7,"parentSeq":6,"requestId":"req-1"}`,
  ];

  it("2.0 收尾协议回合：作者可见答复在 display.completion.message，不因正文为空而丢", async () => {
    expect((await derive(TS20_FINISH_TURN_LINES))?.messages).toEqual([
      { role: "user", content: "打个招呼", timestamp: 105 },
      { role: "assistant", content: "OK", timestamp: 108 },
    ]);
    // 收尾答复优先于同事件的正文块。
    const both = TS20_FINISH_TURN_LINES.map((line) => (
      line.includes('"uuid":"u-8"') ? line.replace('"content":[]', '"content":[{"type":"text","text":"正文"}]') : line
    ));
    expect((await derive(both))?.messages[1]?.content).toBe("OK");
  });

  it("会话协议工具（finish_turn）结果对作者可见时：入卡但不带调用参数，收尾答复不被结果卡吞掉", async () => {
    const visible = TS20_FINISH_TURN_LINES.map((line) => line.replace('"visibility":"model",', ""));
    expect((await derive(visible))?.messages).toEqual([
      { role: "user", content: "打个招呼", timestamp: 105 },
      {
        role: "assistant", content: "OK", timestamp: 108,
        toolExecutions: [{
          id: "call-finish", tool: "finish_turn", label: "finish_turn", status: "completed", result: "OK",
          details: { kind: "turn_completion", status: "answered", message: "OK" }, startedAt: 107, completedAt: 107,
        }],
      },
    ]);
  });

  it("2.0 带附件的用户输入：作者可见正文 = 输入正文 + 附件清单", async () => {
    const line = `{"type":"message","version":1,"sessionId":"s20","seq":3,"timestamp":103,"parentSeq":2,"requestId":"req-20","uuid":"u-3","parentUuid":null,"role":"user","display":{"userInput":{"text":"看看这两份设定","language":"zh","attachments":[{"filename":"a.md"},{"filename":"b.png"}]}},"message":{"role":"user","content":[{"type":"text","text":"[附件]"}]}}`;
    const committed = `{"type":"request_committed","version":1,"sessionId":"s20","seq":4,"timestamp":104,"parentSeq":3,"requestId":"req-20"}`;
    expect((await derive([line, committed]))?.messages[0]?.content).toBe("看看这两份设定\n\n附件：\n- a.md\n- b.png");
    expect((await derive([line.replace('"language":"zh"', '"language":"en"'), committed]))?.messages[0]?.content)
      .toBe("看看这两份设定\n\nAttachments:\n- a.md\n- b.png");
  });

  it("工具结果读面不截断：上限由写入点定，读面原样透出", async () => {
    const longResult = "字".repeat(1500);
    expect((await derive(rustTurn(longResult, false)))?.messages).toEqual([
      { role: "user", content: "读文件", timestamp: 3 },
      {
        role: "assistant", content: "读完了", timestamp: 6,
        toolExecutions: [{
          id: "call-1", tool: "read", label: "读取文件", status: "completed", args: { path: "a.md" },
          result: longResult, startedAt: 4, completedAt: 5,
        }],
      },
    ]);
    const longError = "误".repeat(600);
    expect((await derive(rustTurn(longError, true)))?.messages[1]?.toolExecutions?.[0]).toMatchObject({
      status: "error", error: longError,
    });
  });

  it("2.0 动作授权工具名取动作段；沉浸场景结果以独立卡还原", async () => {
    const head = [
      `{"type":"session_created","version":1,"sessionId":"s20","seq":1,"timestamp":1,"parentSeq":null,"bookId":null,"title":null,"createdAt":1,"updatedAt":1}`,
      `{"type":"message","version":1,"sessionId":"s20","seq":2,"timestamp":2,"parentSeq":1,"requestId":"req-1","uuid":"u-2","parentUuid":null,"role":"user","message":{"role":"user","content":[{"type":"text","text":"往前走"}]}}`,
    ];
    const call = (id: string, name: string) => `{"type":"message","version":1,"sessionId":"s20","seq":3,"timestamp":3,"parentSeq":2,"requestId":"req-1","uuid":"u-3","parentUuid":"u-2","role":"assistant","message":{"role":"assistant","content":[{"type":"toolCall","id":"${id}","name":"${name}","arguments":{"step":"向前"}}]}}`;
    const result = (id: string, name: string, details: string) => `{"type":"message","version":1,"sessionId":"s20","seq":4,"timestamp":4,"parentSeq":3,"requestId":"req-1","uuid":"u-4","parentUuid":"u-3","role":"toolResult","toolCallId":"${id}","message":{"role":"toolResult","toolCallId":"${id}","toolName":"${name}","content":[{"type":"text","text":"你站在门前。"}]${details},"isError":false,"timestamp":4}}`;
    const prose = `{"type":"message","version":1,"sessionId":"s20","seq":5,"timestamp":5,"parentSeq":4,"requestId":"req-1","uuid":"u-5","parentUuid":"u-4","role":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"场景旁白"}]}}`;
    const committed = `{"type":"request_committed","version":1,"sessionId":"s20","seq":6,"timestamp":6,"parentSeq":5,"requestId":"req-1"}`;
    const scene = "interactive_world__immersive_step";

    // 沉浸场景：独立卡（内容为空、收尾正文转入思考），细节透出。
    expect((await derive([...head, call("call-s", scene), result("call-s", scene, `,"details":{"presentation":"immersive-scene"}`), prose, committed]))?.messages)
      .toEqual([
        { role: "user", content: "往前走", timestamp: 2 },
        {
          role: "assistant", content: "", thinking: "场景旁白", timestamp: 5,
          toolExecutions: [{
            id: "call-s", tool: "immersive_step", label: "immersive_step", status: "completed", args: { step: "向前" },
            result: "你站在门前。", details: { presentation: "immersive-scene" }, startedAt: 3, completedAt: 4,
          }],
        },
      ]);

    // 对照：同一工具、结果无 presentation → 普通工具卡，挂到收尾助手消息上。
    expect((await derive([...head, call("call-s", scene), result("call-s", scene, ""), prose, committed]))?.messages[1])
      .toEqual({
        role: "assistant", content: "场景旁白", timestamp: 5,
        toolExecutions: [{
          id: "call-s", tool: "immersive_step", label: "immersive_step", status: "completed", args: { step: "向前" },
          result: "你站在门前。", startedAt: 3, completedAt: 4,
        }],
      });

    // 2.0 长篇动作工具：标签表取动作段。
    const review = "longform__review_chapter";
    expect((await derive([...head, call("call-r", review), result("call-r", review, ""), prose, committed]))?.messages[1]?.toolExecutions?.[0])
      .toMatchObject({ tool: "review_chapter", label: "审查章节" });
  });
});
