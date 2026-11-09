//! R18/386 等待轮：tar 读取解析单测（回放/安全三则/gzip 层）。
import { gzipSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import { untar } from "./tar-read";

/** 手工构造最小 USTAR 归档（与 buildTarArchive 同格式）。 */
function makeTar(entries: Array<{ name: string; content: string; typeflag?: string; prefix?: string; gnuMagic?: boolean; longName?: string }>): Buffer {
  const blocks: Buffer[] = [];
  for (const entry of entries) {
    const header = Buffer.alloc(512, 0);
    Buffer.from(entry.name, "utf-8").copy(header, 0);
    header.write("0000000", 100); // mode
    const size = Buffer.byteLength(entry.content, "utf-8");
    header.write(size.toString(8).padStart(11, "0"), 124);
    header.write(entry.typeflag ?? "0", 156);
    // GNU 头 magic="ustar "+version=" \0"（Rust tar::Header::new_gnu 形态）；
    // 默认 POSIX "ustar\0"+"00"。
    if (entry.gnuMagic) {
      header.write("ustar ", 257, "utf-8");
      header.write(" \0", 263);
    } else {
      header.write("ustar\0", 257, "utf-8");
      header.write("00", 263);
    }
    if (entry.prefix) Buffer.from(entry.prefix, "utf-8").copy(header, 345);
    blocks.push(header);
    const payload = Buffer.from(entry.content, "utf-8");
    blocks.push(payload);
    const padding = (512 - (payload.length % 512)) % 512;
    if (padding > 0) blocks.push(Buffer.alloc(padding));
  }
  blocks.push(Buffer.alloc(1024));
  return Buffer.concat(blocks);
}

/** GNU LongLink（typeflag 'L'）扩展条目：payload=完整名+\0（Rust tar::Builder GNU 长路径形态）。 */
function makeGnuLongLinkTar(longName: string, content: string, truncatedName: string): Buffer {
  return makeTar([
    { name: "././@LongLink", content: `${longName}\0`, typeflag: "L" },
    { name: truncatedName, content },
  ]);
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

  // —— 950 号：跨端备份包互灌（Rust GNU 长路径产出 ↔ POSIX prefix）——

  it("restores GNU LongLink long name entries (Rust export form)", () => {
    const longName = "inkos-backup/works/w-12345678-1234-5678-1234-567812345678/revisions/r-12345678-1234-5678-1234-567812345678/chapter-0001-r2.json";
    expect(longName.length).toBeGreaterThan(100);
    const tar = makeGnuLongLinkTar(longName, "rev body", longName.slice(0, 100));
    const { entries, rejected } = untar(tar);
    expect(rejected).toHaveLength(0); // 'L' 扩展条目不进 rejected——此前整包 400 的患根
    expect(entries).toHaveLength(1);
    expect(entries[0]!.name).toBe(longName);
    expect(new TextDecoder().decode(entries[0]!.bytes)).toBe("rev body");
  });

  it("GNU LongLink payload cannot bypass path guard (final-name check)", () => {
    const tar = makeGnuLongLinkTar("../../evil/escape.txt", "bad", "../../evil/escape.txt");
    const { entries, rejected } = untar(tar);
    expect(entries).toHaveLength(0);
    expect(rejected).toEqual(["../../evil/escape.txt"]);
  });

  it("joins POSIX USTAR prefix field into entry name", () => {
    const tar = makeTar([
      { name: "chapter-0001-r2.json", content: "body", prefix: "inkos-backup/works/w1/revisions/r1" },
    ]);
    const { entries, rejected } = untar(tar);
    expect(rejected).toHaveLength(0);
    expect(entries.map((entry) => entry.name)).toEqual(["inkos-backup/works/w1/revisions/r1/chapter-0001-r2.json"]);
  });

  it("malicious prefix cannot bypass path guard", () => {
    const tar = makeTar([
      { name: "evil.txt", content: "bad", prefix: ".." },
    ]);
    const { entries, rejected } = untar(tar);
    expect(entries).toHaveLength(0);
    expect(rejected).toEqual(["../evil.txt"]);
  });

  it("GNU magic header does not join prefix field", () => {
    const tar = makeTar([
      { name: "plain.md", content: "body", prefix: "should-not-join", gnuMagic: true },
    ]);
    const { entries } = untar(tar);
    expect(entries.map((entry) => entry.name)).toEqual(["plain.md"]);
  });

  it("long name payload counts toward max total bytes", () => {
    const tar = makeGnuLongLinkTar("inkos-backup/" + "d".repeat(200) + "/f.txt", "x", "f.txt");
    expect(() => untar(tar, { maxTotalBytes: 8 })).toThrow(/max total bytes/);
  });

  it("'L' payload coinciding with truncated name does not join prefix (953 来源判定)", () => {
    // 畸形形态：'L' payload 恰等于真条目截断名且真条目带 USTAR prefix——
    // 来源判定下不拼 prefix（值比较会误拼得 prefix+名）。
    const tar = makeTar([
      { name: "truncated-name.json", content: "truncated-name.json\0", typeflag: "L", prefix: "some/dir" },
      { name: "truncated-name.json", content: "body", prefix: "some/dir" },
    ]);
    const { entries, rejected } = untar(tar);
    expect(rejected).toHaveLength(0);
    expect(entries.map((e) => e.name)).toEqual(["truncated-name.json"]);
  });
});
