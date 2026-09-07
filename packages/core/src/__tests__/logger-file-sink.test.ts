import { describe, expect, it } from "vitest";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createFileSink, createLogger } from "../utils/logger.js";

/**
 * 210 号：inkos.log 落盘通道——GET /api/v1/logs 双端读该文件但此前零写入方。
 */
describe("createFileSink", () => {
  const roots: string[] = [];
  const cleanup = async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  };

  it("appends JSON lines with LogEntry shape", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-logsink-"));
    roots.push(root);
    const path = join(root, "inkos.log");
    const logger = createLogger({ tag: "studio", sinks: [createFileSink(path)] });
    logger.warn("连写失败：已完成 1/2 章并落盘");
    logger.error("boom");

    const content = await readFile(path, "utf-8");
    const lines = content.trim().split("\n");
    expect(lines).toHaveLength(2);
    const first = JSON.parse(lines[0]) as { level: string; tag: string; message: string; timestamp: string };
    expect(first).toMatchObject({ level: "warn", tag: "studio", message: "连写失败：已完成 1/2 章并落盘" });
    expect(typeof first.timestamp).toBe("string");
    await cleanup();
  });

  it("swallows write failures without throwing", async () => {
    // 不可写路径（文件当目录用）——静默不抛
    const root = await mkdtemp(join(tmpdir(), "inkos-logsink-"));
    roots.push(root);
    const bad = join(root, "inkos.log", "nested");
    const logger = createLogger({ tag: "studio", sinks: [createFileSink(bad)] });
    expect(() => logger.info("x")).not.toThrow();
    await cleanup();
  });
});
