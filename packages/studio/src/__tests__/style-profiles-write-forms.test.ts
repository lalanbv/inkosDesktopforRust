import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 964 号：档案池文本写入形态端到端——`POST /style-profiles {name, text}` 服务端算统计
 * 指纹落池（文风页「保存为写法档案」入口）。engine-rs ops_routes.rs
 * `save_style_profile_text_form_matches_fingerprint_golden` 同端点同 golden 同断言对偶。
 */
const golden = JSON.parse(
  readFileSync(join(fileURLToPath(new URL(".", import.meta.url)), "golden", "style-fingerprint-vectors.json"), "utf8"),
) as { vectors: Array<{ id: string; text: string; expected: Record<string, unknown> }> };
const vector = (id: string) => golden.vectors.find((item) => item.id === id)!;

describe("style profiles write forms (964)", () => {
  let root: string;
  beforeEach(async () => {
    root = await mkdtemp(join(tmpdir(), "style-profiles-forms-"));
    await writeFile(join(root, "inkos.json"), '{"name":"forms"}', "utf-8");
  });
  afterEach(async () => {
    await rm(root, { recursive: true, force: true });
  });

  const post = async (app: ReturnType<typeof createStudioServer>, body: unknown) => {
    const res = await app.request("/api/v1/style-profiles", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return { status: res.status, body: (await res.json()) as { error?: string; name?: string; profile?: Record<string, unknown> } };
  };

  it("文本形态：服务端指纹逐字段等于 golden（sourceName=档案名），en 显式语言走英文指纹", async () => {
    const app = createStudioServer({} as never, root);
    const cases: Array<[string, string, string | undefined]> = [
      ["热血少年", "zh-dialogue-rhetoric", undefined],
      ["Noir", "en-prose", "en"],
    ];
    for (const [name, id, language] of cases) {
      const res = await post(app, { name, text: vector(id).text, ...(language ? { language } : {}) });
      expect(res.status, id).toBe(200);
      expect(res.body.name).toBe(name);
      const saved = JSON.parse(await readFile(join(root, ".inkos", "style-profiles", `${name}.json`), "utf-8")) as Record<string, unknown>;
      expect(saved, "响应 profile = 落盘全文").toEqual(res.body.profile);
      expect(typeof saved.analyzedAt === "string" && saved.analyzedAt.endsWith("Z")).toBe(true);
      const { analyzedAt: _at, ...fingerprint } = saved;
      expect(fingerprint, id).toEqual({ ...vector(id).expected, sourceName: name });
    }
  });

  it("profile 形态原样落盘；错误面合并消息；不规范名先拒", async () => {
    const app = createStudioServer({} as never, root);
    expect((await post(app, { name: "原形态", profile: { a: 1 } })).status).toBe(200);
    expect(JSON.parse(await readFile(join(root, ".inkos", "style-profiles", "原形态.json"), "utf-8"))).toEqual({ a: 1 });
    for (const payload of [{}, { name: "x" }, { name: "x", text: "   " }, { name: "x", profile: null }, { name: "  ", text: "有正文。" }]) {
      const res = await post(app, payload);
      expect(res.status, JSON.stringify(payload)).toBe(400);
      expect(res.body.error, JSON.stringify(payload)).toBe("name and profile (or text) are required");
    }
    const evil = await post(app, { name: "../evil", text: "有正文。" });
    expect(evil.status).toBe(400);
    expect(evil.body.error).toBe("invalid profile name");
  });
});
