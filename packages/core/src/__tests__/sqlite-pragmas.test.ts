import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { LocalSearchIndex } from "../retrieval/local-search.js";
import { MemoryDB } from "../state/memory-db.js";
import { PlayDB } from "../play/play-db.js";

// 631 号：SQLite 连接配置锁定。busy_timeout 是 per-connection 值（不持久化），
// 只能从本连接读回；并发等待的行为机制可证伪证明在 Rust 侧
// local_search 并发持锁测试（engine-rs/src/utils/local_search.rs），双端同值 5000。

/** db 字段为类私有（运行时真实存在），测试经窄化 cast 读取连接做 PRAGMA 断言。 */
function rawDb(instance: unknown): { prepare(sql: string): { get(): unknown } } {
  return (instance as { db: { prepare(sql: string): { get(): unknown } } }).db;
}

describe("SQLite connection pragmas (631)", () => {
  const dir = mkdtempSync(join(tmpdir(), "inkos-pragmas-"));

  afterAll(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("LocalSearchIndex sets busy_timeout=5000 and WAL on disk databases", () => {
    const index = new LocalSearchIndex(join(dir, "ls", "memory.db"));
    try {
      const busy = rawDb(index).prepare("PRAGMA busy_timeout").get() as { timeout: number };
      expect(busy.timeout).toBe(5000);
      const journal = rawDb(index).prepare("PRAGMA journal_mode").get() as { journal_mode: string };
      expect(journal.journal_mode).toBe("wal");
    } finally {
      index.close();
    }
  });

  it("MemoryDB sets busy_timeout=5000 and WAL", () => {
    // MemoryDB 不创建父目录（生产调用方保证 story/ 存在），测试先建。
    mkdirSync(join(dir, "story"), { recursive: true });
    const db = new MemoryDB(dir);
    try {
      const busy = rawDb(db).prepare("PRAGMA busy_timeout").get() as { timeout: number };
      expect(busy.timeout).toBe(5000);
      const journal = rawDb(db).prepare("PRAGMA journal_mode").get() as { journal_mode: string };
      expect(journal.journal_mode).toBe("wal");
    } finally {
      db.close();
    }
  });

  it("PlayDB sets busy_timeout=5000 and WAL (631 备案清偿)", () => {
    const db = new PlayDB(join(dir, "play-run"));
    try {
      const busy = rawDb(db).prepare("PRAGMA busy_timeout").get() as { timeout: number };
      expect(busy.timeout).toBe(5000);
      const journal = rawDb(db).prepare("PRAGMA journal_mode").get() as { journal_mode: string };
      expect(journal.journal_mode).toBe("wal");
    } finally {
      db.close();
    }
  });
});
