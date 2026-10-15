import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 707 号：选后再析两面回收（G14a/335）端到端——POST /radar/rankings（免费
 * 扫榜不落历史）+ POST /radar/analyze（勾选范围分析 + 落历史 + SSE 广播）。
 * fork 原实现逐字，engine-rs ops_routes.rs post_radar_rankings/analyze 同水位
 * 对偶。UI（RadarView）在位消费本两面——node 回退腿此前 404 断链。
 */
describe("radar selection routes (707: G14a/335)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "radar-selection-"));
  });
  afterEach(async () => {
    vi.unstubAllGlobals();
    await rm(root, { recursive: true, force: true });
  });

  it("rankings: 双源 stub fetch → 200 {rankings} 且不落历史", async () => {
    // hermetic：stub globalThis.fetch——榜单源容错语义在位时按响应解析
    vi.stubGlobal("fetch", async (input: string | URL | Request) => {
      const url = String(input);
      if (url.includes("fanqiesdk")) {
        return new Response(JSON.stringify({
          data: { result: [{ book_name: "镜花缘", author: "甲", category: "玄幻" }] },
        }), { status: 200 });
      }
      if (url.includes("qidian.com")) {
        return new Response('<a href="//book.qidian.com/info/1" >剑来</a>', { status: 200 });
      }
      return new Response("", { status: 404 });
    });

    const app = createStudioServer({} as never, root);
    const res = await app.request("/api/v1/radar/rankings", { method: "POST" });
    expect(res.status).toBe(200);
    const body = await res.json() as { rankings: ReadonlyArray<{ platform: string; entries: ReadonlyArray<{ title: string }> }> };
    expect(Array.isArray(body.rankings)).toBe(true);
    const fanqie = body.rankings.find((r) => r.platform === "番茄小说");
    expect(fanqie?.entries.map((e) => e.title)).toContain("镜花缘");
    const qidian = body.rankings.find((r) => r.platform === "起点中文网");
    expect(qidian?.entries.map((e) => e.title)).toContain("剑来");
  });

  it("analyze: 空 selection 走全量 + 无 LLM 配置降级 500（radar:error 广播路径）", async () => {
    const app = createStudioServer({} as never, root);
    // 临时根无 LLM 配置 → buildPipelineConfig 失败 → 500 + radar:error 广播
    //（fork 原实现 catch 语义；路由在位证明 = 非 404）。
    const res = await app.request("/api/v1/radar/analyze", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ selection: { platforms: [], categories: [] } }),
    });
    expect(res.status).toBe(500);
    const body = await res.json() as { error: string };
    expect(typeof body.error).toBe("string");
  });

  it("scan 既有面回归锁定：selection 缺省仍走 2.0 harness 全量扫描", async () => {
    const app = createStudioServer({} as never, root);
    // 同一临时根无 LLM 配置 → 500（既有 /radar/scan 行为不变——707 嫁接
    // 只增不改，daemon/tick 的 submitStructured 路径原样）。
    const res = await app.request("/api/v1/radar/scan", { method: "POST" });
    expect(res.status).toBe(500);
  });
});
