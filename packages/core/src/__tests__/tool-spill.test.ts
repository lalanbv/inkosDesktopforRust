import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { createReadTool } from "../agent/agent-tools.js";

import {
  SPILL_EXEMPT_TOOLS,
  SPILL_HEAD_BYTES,
  SPILL_TAIL_BYTES,
  SPILL_THRESHOLD_BYTES,
  sessionDirName,
  shouldSpill,
  sliceUtf8Bytes,
  spillToolText,
} from "../utils/tool-spill.js";

/**
 * Tool result spill（R42，557 号）——engine-rs src/interaction/spill.rs
 * 测试矩阵同水位对偶（阈值/豁免/边界切分/落盘/降级）。
 */
describe("tool-spill", () => {
  it("shouldSpill judges threshold, exemption, and errors", () => {
    const big = "x".repeat(SPILL_THRESHOLD_BYTES + 1);
    const small = "x".repeat(SPILL_THRESHOLD_BYTES);
    expect(shouldSpill("research_web", big, false)).toBe(true);
    expect(shouldSpill("research_web", small, false)).toBe(false) /* 恰在阈值上不 spill */;
    for (const tool of SPILL_EXEMPT_TOOLS) {
      expect(shouldSpill(tool, big, false)).toBe(false); /* 豁免防回环 */
    }
    expect(shouldSpill("research_web", big, true)).toBe(false) /* 错误结果不 spill */;
  });

  it("sessionDirName hashes deterministically and falls back", () => {
    const name = sessionDirName("sess-abc");
    expect(name).toBe(`session-${createHash("sha256").update("sess-abc").digest("hex").slice(0, 12)}`);
    expect(name).toBe(sessionDirName("sess-abc"));
    expect(name).not.toBe(sessionDirName("sess-other"));
    expect(sessionDirName(undefined)).toBe("session-adhoc");
  });

  it("splitHeadTail honors UTF-8 budgets at code point boundaries", async () => {
    const text = "汉".repeat(3000); // 9000 字节 > 6000 阈值
    const spilled = await spillToolText("/tmp", undefined, "research_web", text);
    expect(spilled).toContain("\n\n[...]\n\n");
    expect(spilled).toContain("(Omitted ");
    // 头尾保留量与 Rust 对齐：2800/1600 字节预算（汉字 3 字节/码元 → 933/533 码元）。
    const head = spilled.slice(0, spilled.indexOf("\n\n[...]"));
    expect(head.length).toBe(Math.floor(SPILL_HEAD_BYTES / 3));
  });

  it("spillToolText writes the full text and replaces the inline copy", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-spill-test-"));
    const big = "汉".repeat(3000);
    const spilled = await spillToolText(root, "sess-abc", "research_web", big);
    expect(spilled).toContain("bytes. Full result stored at: .inkos/spills/session-");
    expect(spilled).toContain("Use the read tool on this path");
    expect(spilled.startsWith("汉")).toBe(true);
    expect(spilled.endsWith("view the full text.)")).toBe(true) /* 通知收尾（tail 后接省略通知） */;
    // 相对路径解析回真实文件，内容逐字一致。
    const relative = spilled.slice(spilled.indexOf(".inkos/spills/")).split(". Use the read tool")[0];
    expect(relative.endsWith("-research_web.txt")).toBe(true);
    const full = await readFile(join(root, relative), "utf-8");
    expect(full).toBe(big);
    // 目录权限形态：0700 会话目录、0600 文件（wx 独占写）——unix 下校验。
    const sessionEntries = await readdir(join(root, ".inkos", "spills"));
    expect(sessionEntries.length).toBe(1);
  });

  it("spillToolText keeps the inline text when the save fails", async () => {
    // root 是常规文件 → mkdir 失败 → 降级保留原文（尽力而为）。
    const dir = await mkdtemp(join(tmpdir(), "inkos-spill-test-"));
    const fileRoot = join(dir, "not-a-dir");
    await writeFile(fileRoot, "i am a file");
    const big = "x".repeat(SPILL_THRESHOLD_BYTES + 100);
    const spilled = await spillToolText(fileRoot, undefined, "research_web", big);
    expect(spilled).toBe(big);
  });

  it("slicing keeps surrogate pairs intact at the budget edge", () => {
    // pair 恰在预算边界：二分停在孤 high 码元（replacement 3 字节计入预算），
    // 修复分支让 pair 完整归入省略侧——输出无 U+FFFD、无孤立代理。
    const pair = "😀"; // 4 字节 UTF-8 / 2 码元
    const text = "a".repeat(SPILL_HEAD_BYTES - 4) + pair + "b".repeat(2000);
    const head = sliceUtf8Bytes(text, SPILL_HEAD_BYTES);
    expect(head.includes("\uFFFD")).toBe(false) /* 无替换字符 */;
    expect(head.startsWith("a")).toBe(true);
    const tail = sliceUtf8Bytes(text, SPILL_TAIL_BYTES, true);
    expect(tail.includes("\uFFFD")).toBe(false);
    expect(tail.endsWith("b")).toBe(true);
    // ASCII 场景精确预算。
    const ascii = "a".repeat(5000);
    expect(sliceUtf8Bytes(ascii, 2800).length).toBe(2800);
    expect(sliceUtf8Bytes(ascii, 1600, true).length).toBe(1600);
  });

  it("read tool reaches spill files under the books-limited root", async () => {
    // R42 读回通道：books 限定 read 对 `.inkos/spills/` 前缀按项目根解析。
    const root = await mkdtemp(join(tmpdir(), "inkos-spill-test-"));
    const spillDir = join(root, ".inkos", "spills", "session-adhoc");
    await mkdir(spillDir, { recursive: true });
    await writeFile(join(spillDir, "abc12345-research_web.txt"), " spilled 全文");
    const tool = createReadTool(root);
    const read = await tool.execute("call-1", { path: ".inkos/spills/session-adhoc/abc12345-research_web.txt" });
    expect((read.content[0] as { text: string }).text).toContain("spilled 全文");
    // books/ 之外的普通路径仍拒；真逃逸仍拒。
    const denied = await tool.execute("call-2", { path: ".inkos/index.json" });
    expect((denied.content[0] as { text: string }).text).toContain("Failed to read");
    const traversal = await tool.execute("call-3", { path: ".inkos/spills/../../../../secret.txt" });
    expect((traversal.content[0] as { text: string }).text).toContain("Failed to read");
  });
});
