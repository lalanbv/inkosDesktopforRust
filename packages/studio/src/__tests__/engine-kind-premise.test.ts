import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { probeEngineKind } from "../hooks/use-engine-kind";

/**
 * 1668：服务腿判型前提——Node 回退腿对 /api/v1/health 必须是 404。
 *
 * 62 号刻意保留的 Rust 超集端点。TS 腿一旦「顺手补上」该路由，UI 会把回退腿
 * 误判成产品引擎，反AI规则 / 经验条目 / best-of-N 的不生效标明全部消失
 * （空开关回归）。真探测逻辑打真服务，非 mock 回声。
 */
let root: string;
let app: ReturnType<typeof createStudioServer>;

beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), "engine-kind-"));
  app = createStudioServer({} as never, root);
});
afterEach(async () => {
  await rm(root, { recursive: true, force: true });
});

it("the Node fallback leg answers 404 on /api/v1/health, so the probe resolves to node", async () => {
  const kind = await probeEngineKind((input) => app.request(String(input)) as unknown as Promise<Response>);
  expect(kind).toBe("node");
});
