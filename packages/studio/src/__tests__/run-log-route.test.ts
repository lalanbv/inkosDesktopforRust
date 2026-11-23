import { globalRunLog, recordRunLogCall } from "@actalk/inkos-core";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * R26 运行遥测路由（403 号；701 号回收；1663 号回座）：GET /run-log 只读投影。
 *
 * 1663 号实证：TS 腿写入点自 688 号合并起缺失（model-chain.ts 退役带走唯一
 * append），端点在位却恒空——写入点复座后本文件锁三面：投影形状与 limit
 * 截尾、no-store、以及「写入点 → 端点」的同进程贯通（经 core 单例
 * recordRunLogCall 落一条，端点必须读出；写入点本体由 core
 * run-log-writer.test 直锁两条 LLM 传输边界）。
 */
describe("run-log route (R26/403, 1663 回座)", () => {
  function seedApp() {
    const root = mkdtempSync(join(tmpdir(), "run-log-route-"));
    return createStudioServer({} as never, root);
  }

  it("projects the shared buffer: shape, aggregates and limit tail", async () => {
    const app = seedApp();
    const before = globalRunLog.snapshot().totalAppended;
    recordRunLogCall({ agent: "route-probe", model: "m-a", startedAt: Date.now() - 3, ok: true });
    recordRunLogCall({ agent: "route-probe", model: "m-b", startedAt: Date.now(), ok: false, round: 2, errorKind: "transient" });

    const full = await app.request("/api/v1/run-log");
    expect(full.status).toBe(200);
    expect(full.headers.get("cache-control")?.includes("no-store")).toBe(true);
    const body = (await full.json()) as {
      total: number; kept: number; tookOverCount: number; failureCount: number;
      entries: ReadonlyArray<Record<string, unknown>>;
    };
    for (const key of ["total", "kept", "tookOverCount", "failureCount", "entries"]) {
      expect(key in body, `missing key ${key}`).toBe(true);
    }
    expect(body.total).toBe(before + 2);
    expect(body.kept).toBe(Math.min(before + 2, 200));
    const mine = body.entries.filter((entry) => entry.agent === "route-probe");
    expect(mine.map((entry) => [entry.model, entry.ok, entry.errorKind])).toEqual([
      ["m-a", true, null],
      ["m-b", false, "transient"],
    ]);

    // limit = 最近 N 条（时间升序不变）
    const tail = await app.request("/api/v1/run-log?limit=1");
    const tailBody = (await tail.json()) as { total: number; entries: ReadonlyArray<{ model: string }> };
    expect(tailBody.total).toBe(body.total);
    expect(tailBody.entries).toHaveLength(1);
    expect(tailBody.entries[0]!.model).toBe("m-b");

    // 非正/非法 limit 回退全量
    const fallback = await app.request("/api/v1/run-log?limit=abc");
    expect(((await fallback.json()) as { entries: unknown[] }).entries.length).toBe(body.entries.length);
  });
});
