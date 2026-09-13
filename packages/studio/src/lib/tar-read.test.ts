//! R18/386 等待轮：tar 读取解析单测（回放/安全三则/gzip 层）。
import { gzipSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import { untar } from "./tar-read";

/** 手工构造最小 USTAR 归档（与 buildTarArchive 同格式）。 */
function makeTar(entries: Array<{ name: string; content: string; typeflag?: string }>): Buffer {
  const blocks: Buffer[] = [];
  for (const entry of entries) {
    const header = Buffer.alloc(512, 0);
    Buffer.from(entry.name, "utf-8").copy(header, 0);
    header.write("0000000", 100); // mode
    const size = Buffer.byteLength(entry.content, "utf-8");
    header.write(size.toString(8).padStart(11, "0"), 124);
    header.write(entry.typeflag ?? "0", 156);
    header.write("ustar\0", 257, "utf-8");
    header.write("00", 263);
    blocks.push(header);
    const payload = Buffer.from(entry.content, "utf-8");
    blocks.push(payload);
    const padding = (512 - (payload.length % 512)) % 512;
    if (padding > 0) blocks.push(Buffer.alloc(padding));
  }
  blocks.push(Buffer.alloc(1024));
  return Buffer.concat(blocks);
}

describe("tar read (R18 预研)", () => {
  it("parses entries and skips directory items", () => {
    const tar = makeTar([
      { name: "books/b1/story/current_state.md", content: "state body" },
      { name: "books/b1/chapters", content: "", typeflag: "5" },
      { name: "inkos.json", content: "{}" },
    ]);
    const { entries, rejected } = untar(tar);
    expect(entries.map((entry) => entry.name)).toEqual([
      "books/b1/story/current_state.md",
      "inkos.json",
    ]);
    expect(rejected).toHaveLength(0);
    expect(new TextDecoder().decode(entries[0]!.bytes)).toBe("state body");
  });

  it("rejects path traversal entries without aborting", () => {
    const tar = makeTar([
      { name: "../evil.txt", content: "bad" },
      { name: "safe.md", content: "good" },
    ]);
    const { entries, rejected } = untar(tar);
    expect(entries.map((entry) => entry.name)).toEqual(["safe.md"]);
    expect(rejected).toEqual(["../evil.txt"]);
  });

  it("accepts gzip layer input", () => {
    const tar = makeTar([{ name: "a.md", content: "hello" }]);
    const gz = gzipSync(tar);
    const { entries } = untar(new Uint8Array(gz));
    expect(entries).toHaveLength(1);
    expect(new TextDecoder().decode(entries[0]!.bytes)).toBe("hello");
  });
});
