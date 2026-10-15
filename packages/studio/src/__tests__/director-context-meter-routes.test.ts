import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";
import { formatResumeHint } from "@actalk/inkos-core";

/**
 * 713 号：导演域三面 + context-meter 观测面回收端到端——G6/353-354 号导演
 * 会话（GET/PUT patch 六键白名单合并 + updatedAt 对齐 Rust utc_now_ms 数字
 * 毫秒）与方向候选 LLM 面（Runner.generateDirections），R41/564 号会话末轮
 * token 计量快照（缺参 400 / 无记录 404 typed 错误体双端同形）。Rust 对偶
 * engine-rs ops_routes.rs（get_director/put_director/post_direction_candidates）
 * 与 agent_route.rs（get_context_meter/context_meter_registry）。
 */
describe("director + context-meter recycle routes (713)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "director-meter-recycle-"));
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  it("director GET: 缺省 {session:null} + resumeAdvice 注入 + 会话文件回读", async () => {
    const app = createStudioServer({} as never, root);

    // GET：无会话文件 → session=null，resumeAdvice 非空（golden 同源文案）。
    const missing = await app.request("/api/v1/books/b1/director");
    expect(missing.status).toBe(200);
    const payload = (await missing.json()) as {
      session: Record<string, unknown> | null;
      savedChapters: number;
      resumeAdvice: string;
    };
    expect(payload.session).toBeNull();
    expect(payload.savedChapters).toBe(0);
    expect(payload.resumeAdvice).toBe(formatResumeHint(0, "en"));
    expect(payload.resumeAdvice.length).toBeGreaterThan(0);

    // GET：会话文件在位 → 原样解析（selectedDirection 等 patch 键透传）。
    const dir = join(root, ".inkos", "director");
    await mkdir(dir, { recursive: true });
    const session = {
      bookId: "b1",
      runMode: "range",
      stage: "directions",
      inspiration: { premise: "镜中图书馆", keywords: ["镜像", "藏书"] },
      selectedDirection: { title: "镜花园" },
    };
    await writeFile(join(dir, "b1.json"), JSON.stringify(session, null, 2), "utf-8");
    const round = await app.request("/api/v1/books/b1/director");
    expect(round.status).toBe(200);
    const loaded = (await round.json()) as { session: Record<string, unknown> | null };
    expect(loaded.session).toEqual(session);

    // GET：坏载荷 → session=null（与 Rust serde 失败 None 同水位，静默）。
    await writeFile(join(dir, "b1.json"), "{broken", "utf-8");
    const corrupt = await app.request("/api/v1/books/b1/director");
    expect(((await corrupt.json()) as { session: unknown }).session).toBeNull();
  });

  it("director PUT: patch 白名单合并 + runMode 校验 400 + updatedAt 数字毫秒", async () => {
    const app = createStudioServer({} as never, root);

    // PUT：非法 runMode → 400（Rust 白名单同水位）。
    const badMode = await app.request("/api/v1/books/b1/director", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ patch: { runMode: "turbo" } }),
    });
    expect(badMode.status).toBe(400);

    // PUT：合法 patch → 白名单键合并，非白名单键丢弃（478 号整体替换语义）。
    const put = await app.request("/api/v1/books/b1/director", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        patch: {
          runMode: "ready-stop",
          stage: "inspiration",
          inspiration: { premise: "镜中图书馆", keywords: ["镜像"] },
          hijacked: true,
        },
      }),
    });
    expect(put.status).toBe(200);
    const written = (await put.json()) as { ok: boolean; session: Record<string, unknown> };
    expect(written.ok).toBe(true);
    expect(written.session.stage).toBe("inspiration");
    expect(written.session.runMode).toBe("ready-stop");
    expect(written.session.hijacked).toBeUndefined();
    expect(written.session.bookId).toBe("b1");
    expect(typeof written.session.updatedAt).toBe("number");

    // GET 回读：roundtrip（709 号教训——写入成功 ≠ 产物可再消费）。
    const round = await app.request("/api/v1/books/b1/director");
    const loaded = (await round.json()) as { session: Record<string, unknown> | null };
    expect(loaded.session?.stage).toBe("inspiration");
    expect(loaded.session?.inspiration).toEqual({ premise: "镜中图书馆", keywords: ["镜像"] });

    // PUT：patch 非对象 → 跳过合并与 updatedAt（对齐 Rust as_object None 分支，
    // 仍 200 落盘现值）。
    const garbage = await app.request("/api/v1/books/b1/director", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ patch: "not-an-object" }),
    });
    expect(garbage.status).toBe(200);
    const untouched = (await garbage.json()) as { session: Record<string, unknown> };
    expect(untouched.session.stage).toBe("inspiration");
    expect(typeof untouched.session.updatedAt).toBe("number");
  });

  it("directions POST: premise 缺失 400 + 无 LLM 路由在位 500", async () => {
    const app = createStudioServer({} as never, root);

    // 空载荷（缺 premise）→ 400（守卫链先于 LLM）。
    const noPremise = await app.request("/api/v1/director/directions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ inspiration: { premise: "   " } }),
    });
    expect(noPremise.status).toBe(400);
    expect(((await noPremise.json()) as { error: string }).error).toContain("premise");

    // 合法灵感但测试环境无 LLM → 500（路由在位；LLM 面状态码口径同 708 号）。
    // 回归锁（713 号）：premise-only（keywords 缺席）必须先过 InspirationCardSchema
    // 归一——fork 旧版 `as never` 裸传时 buildDirectionCandidatesPrompt 读
    // keywords.length 即炸（TypeError），与「LLM 未配置」失败形态混淆；归一后
    // 失败面只剩 LLM 链（错误体非 TypeError）。
    const noLlm = await app.request("/api/v1/director/directions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ inspiration: { premise: "镜中图书馆" }, count: 3 }),
    });
    expect(noLlm.status).toBe(500);
    const noLlmBody = (await noLlm.json()) as { error: string };
    expect(noLlmBody.error).toBeTruthy();
    expect(noLlmBody.error).not.toContain("TypeError");
  });

  it("context-meter GET: 缺参 400 / 未知会话 404（typed 错误体双端同形）", async () => {
    const app = createStudioServer({} as never, root);

    // 缺 sessionId → 400 INVALID_SESSION_ID。
    const noSession = await app.request("/api/v1/context-meter");
    expect(noSession.status).toBe(400);
    expect(await noSession.json()).toEqual({
      error: { code: "INVALID_SESSION_ID", message: "sessionId is required" },
    });

    // 空白 sessionId → 同 400（trim 口径与 Rust filter 非空同水位）。
    const blank = await app.request("/api/v1/context-meter?sessionId=%20%20");
    expect(blank.status).toBe(400);

    // 未知会话 → 404 SESSION_NOT_FOUND。
    const unknown = await app.request("/api/v1/context-meter?sessionId=no-such-session");
    expect(unknown.status).toBe(404);
    expect(await unknown.json()).toEqual({
      error: {
        code: "SESSION_NOT_FOUND",
        message: "no context meter snapshot for this session",
      },
    });
  });
});
