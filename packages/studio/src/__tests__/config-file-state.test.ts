import { readFileSync } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { createStudioServer } from "../api/server.js";

/**
 * 配置文件状态契约（inkos.json 缺失 / 损坏 / 根不是对象）——Node 腿侧。
 *
 * - **文件缺失**：读面降级为「未配置」；服务配置写入即新建。
 * - **文件损坏**（读不出 / 无法解析 / 根不是对象）：读面同样降级；**写入一律
 *   拒绝**——文件逐字节不动。
 * - **诊断面** `GET /api/v1/doctor` 的 `inkosJson` = 文件存在且可解析为对象。
 * - **落盘失败**必须报错：不得回 200，也不得继续动 secrets.json。
 * - **落盘是原子替换写**：崩溃只会留下旧文件或完整的新文件。
 *
 * 写入面 × 文件状态的「状态码 + 响应体」逐格取自两腿共用的对照表
 * （`golden/config-write-parity.json`）；产品引擎侧
 * `engine-rs/tests/config_file_state_contract.rs` 读同一文件——任何一腿漂移
 * 都会在自己这边变红。
 *
 * 取证（修复前）：缺失时 `budget-status` 与 `DELETE /services/:service` 500 并把
 * 绝对路径原样回显；损坏时三个读面 500（产品引擎 200 降级）；`/doctor` 把损坏
 * 文件报成健康；数组根的 `inkos.json` 上 `/project` 族 9 个写入点回 200
 * `{ok:true}` 却什么都没存（产品引擎 500）；`PUT /project`、`POST /project/language`
 * 回显绝对路径 / V8 报错原文。
 */
const here = dirname(fileURLToPath(import.meta.url));
interface Shape {
  readonly status: number;
  readonly response: unknown;
}
interface Row {
  readonly method: string;
  readonly path: string;
  readonly body: string | null;
  readonly env?: boolean;
  readonly expect: Record<string, string | null>;
}
const table = JSON.parse(
  readFileSync(join(here, "golden/config-write-parity.json"), "utf-8"),
) as {
  shapes: Record<string, Shape>;
  states: Record<string, string | null>;
  matrix: Row[];
  cases: Array<{ name: string; config: string | null; seedBook?: boolean } & Omit<Row, "expect"> & { shape: string }>;
  bodyCases: {
    config: string;
    secrets: boolean;
    shape: string;
    requests: Array<Pick<Row, "method" | "path" | "body">>;
  };
};

const CORRUPT = "{ not json";
const CONTRACT_NOT_OBJECT = {
  error: {
    code: "PROJECT_CONFIG_INVALID",
    message: "Refusing to overwrite inkos.json: its root is not a JSON object. Fix or remove the file, then retry.",
  },
};

/** 测试凭据一律 env 注入 + 非凭据回退。 */
const testKey = () => process.env.INKOS_TEST_LLM_KEY ?? "test-key-placeholder";

const json = (method: string, body?: unknown) => ({
  method,
  headers: { "Content-Type": "application/json" },
  ...(body === undefined ? {} : { body: JSON.stringify(body) }),
});

/** `/project` 族里直接落盘 inkos.json 的全部写入点。 */
const PROJECT_WRITES: Array<[string, string, unknown?]> = [
  ["PUT", "/api/v1/project", { language: "en" }],
  ["POST", "/api/v1/project/language", { language: "en" }],
  ["PUT", "/api/v1/project/chapter-review-mode", { mode: "manual" }],
  ["PUT", "/api/v1/project/writing-goal", { dailyGoalWords: 500 }],
  ["PUT", "/api/v1/project/default-model", { defaultModel: "m1" }],
  ["PUT", "/api/v1/project/research-search", { researchSearch: {} }],
  ["PUT", "/api/v1/project/detection", { detection: null }],
  ["PUT", "/api/v1/project/model-overrides", { overrides: {} }],
  ["PUT", "/api/v1/project/notify", { channels: [] }],
];

const VALID_CONFIG = JSON.stringify({
  name: "x", version: "0.1.0", language: "zh",
  llm: { services: [], service: "custom:X", defaultModel: "m1" },
});

/** 可导入的项目 `.env`。 */
async function writeImportableEnv(root: string): Promise<void> {
  await writeFile(
    join(root, ".env"),
    `INKOS_LLM_API_KEY=${testKey()}\nINKOS_LLM_BASE_URL=http://127.0.0.1:9/v1\n`,
  );
}

describe("config file state contract (inkos.json missing / corrupt)", () => {
  const roots: string[] = [];
  afterEach(async () => {
    for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true });
  });
  async function freshRoot(config?: string): Promise<string> {
    const root = await mkdtemp(join(tmpdir(), "inkos-config-state-"));
    roots.push(root);
    if (config !== undefined) await writeFile(join(root, "inkos.json"), config);
    return root;
  }
  const configPath = (root: string) => join(root, "inkos.json");
  const secretsPath = (root: string) => join(root, ".inkos", "secrets.json");
  /** 配置文件当前内容；不存在时回错误码（「仍然不存在」也是一种不动）。 */
  const readConfig = async (root: string): Promise<string> => {
    try {
      return await readFile(configPath(root), "utf-8");
    } catch (error) {
      return (error as NodeJS.ErrnoException).code ?? String(error);
    }
  };

  it("对照表·矩阵：13 个写入点 × 7 种文件状态，拒绝格逐字命中、文件逐字节不动", async () => {
    expect(Object.keys(table.states)).toHaveLength(7);
    expect(table.matrix).toHaveLength(14);
    let checked = 0;
    for (const row of table.matrix) {
      // 每行必须列全全部状态：状态名写错会让那一格被静默跳过。
      expect(Object.keys(row.expect).sort(), row.path).toEqual(Object.keys(table.states).sort());
      for (const [state, content] of Object.entries(table.states)) {
        const shape = row.expect[state];
        if (shape === null) continue;
        const label = `${row.method} ${row.path} [${state}]`;
        const root = await freshRoot(content ?? undefined);
        if (row.env) await writeImportableEnv(root);
        const app = createStudioServer({} as never, root);
        const before = await readConfig(root);

        const response = await app.request(row.path, {
          method: row.method,
          headers: { "Content-Type": "application/json" },
          ...(row.body === null ? {} : { body: row.body }),
        });
        expect(response.status, label).toBe(table.shapes[shape].status);
        expect(await response.json(), label).toEqual(table.shapes[shape].response);
        expect(await readConfig(root), `${label}：文件必须逐字节不动`).toBe(before);
        checked += 1;
      }
    }
    // 4 × 6（服务配置族：缺失即新建，不在本表）+ 10 × 7（`/project` 族）：表被
    // 删行 / 删格时立刻可见。
    expect(checked).toBe(94);
  });

  it("对照表·专项：llm 段形态、请求体不是 JSON、书级审稿模式在坏配置上的读面", async () => {
    expect(table.cases.length).toBeGreaterThanOrEqual(7);
    for (const row of table.cases) {
      const root = await freshRoot(row.config ?? undefined);
      // 书级路由的夹具：一本 2.0 规范布局的书（works/<id>/source/book.json）。
      if (row.seedBook) {
        await mkdir(join(root, "works", "b1", "source"), { recursive: true });
        await writeFile(join(root, "works", "b1", "source", "book.json"), JSON.stringify({ id: "b1", title: "镜花缘" }));
      }
      const app = createStudioServer({} as never, root);
      const before = await readConfig(root);

      const response = await app.request(row.path, {
        method: row.method,
        headers: { "Content-Type": "application/json" },
        ...(row.body === null ? {} : { body: row.body }),
      });
      const expected = table.shapes[row.shape];
      expect(response.status, row.name).toBe(expected.status);
      expect(await response.json(), row.name).toEqual(expected.response);
      expect(await readConfig(root), `${row.name}：文件必须逐字节不动`).toBe(before);
    }
  });

  it("对照表·畸形请求体：需要模型的面（已配置项目）与产品引擎同形", async () => {
    const group = table.bodyCases;
    expect(group.secrets).toBe(true);
    expect(group.requests).toHaveLength(6);
    for (const row of group.requests) {
      const root = await freshRoot(group.config);
      // 密钥只走 secrets.json、不带 .env（.env 会被识别成 env 配置形态）。
      await mkdir(join(root, ".inkos"), { recursive: true });
      await writeFile(secretsPath(root), JSON.stringify({ services: { "custom:Mock": { apiKey: testKey() } } }));
      const app = createStudioServer({} as never, root);
      const before = await readConfig(root);

      const response = await app.request(row.path, {
        method: row.method,
        headers: { "Content-Type": "application/json" },
        ...(row.body === null ? {} : { body: row.body }),
      });
      const expected = table.shapes[group.shape];
      expect(response.status, row.path).toBe(expected.status);
      expect(await response.json(), row.path).toEqual(expected.response);
      expect(await readConfig(root), `${row.path}：文件必须逐字节不动`).toBe(before);
    }
  });

  it("文件缺失：四个服务配置写入点照常成功（写入即新建）", async () => {
    const root = await freshRoot();
    await writeImportableEnv(root);
    const app = createStudioServer({} as never, root);

    const created = await app.request("/api/v1/services/config", json("PUT", { defaultModel: "m1" }));
    expect(created.status).toBe(200);
    expect(JSON.parse(await readFile(configPath(root), "utf-8")).llm).toMatchObject({ defaultModel: "m1" });

    for (const [method, path, body] of [
      ["DELETE", "/api/v1/services/custom:X", undefined],
      ["POST", "/api/v1/services/config/import-env", undefined],
      ["PUT", "/api/v1/cover/config", { service: "kkaiapi" }],
    ] as const) {
      const response = await app.request(path, json(method, body));
      expect(response.status, `${method} ${path}: ${await response.clone().text()}`).toBe(200);
    }
  });

  it("缺失与损坏：读面一律降级为「未配置」", async () => {
    for (const content of [undefined, CORRUPT]) {
      const root = await freshRoot(content);
      const app = createStudioServer({} as never, root);

      const services = await app.request("/api/v1/services/config");
      expect(services.status).toBe(200);
      expect(await services.json()).toMatchObject({ services: [], service: null, defaultModel: null });
      const cover = await app.request("/api/v1/cover/config");
      expect(cover.status).toBe(200);
      expect(await cover.json()).toMatchObject({ service: null, configured: false });
      const budget = await app.request("/api/v1/usage/budget-status");
      expect(budget.status).toBe(200);
      expect((await budget.json()).statuses).toEqual([]);
      // 降级读不改文件：损坏的仍是损坏，缺失的仍不存在（由写入面负责新建）。
      expect(await readConfig(root)).toBe(content ?? "ENOENT");
    }
  });

  it("诊断面：inkosJson = 文件存在且可解析为对象", async () => {
    for (const [content, healthy] of [
      [undefined, false],
      [CORRUPT, false],
      ["[]\n", false],
      [JSON.stringify({ name: "x" }), true],
    ] as const) {
      const root = await freshRoot(content);
      const app = createStudioServer({} as never, root);
      const response = await app.request("/api/v1/doctor");
      expect(response.status).toBe(200);
      expect((await response.json()).inkosJson, `inkos.json = ${content}`).toBe(healthy);
    }
  });

  it("平铺错误端点：不回显绝对路径；文件可用时照常合并", async () => {
    for (const content of [undefined, CORRUPT, "[1]"]) {
      const root = await freshRoot(content);
      const app = createStudioServer({} as never, root);

      for (const [method, path, body] of [
        ["PUT", "/api/v1/project", { language: "zh" }],
        ["POST", "/api/v1/project/language", { language: "zh" }],
      ] as const) {
        const response = await app.request(path, json(method, body));
        expect(response.status, `${method} ${path}`).toBe(500);
        const text = await response.text();
        expect(text, `${method} ${path}`).toBe(JSON.stringify({ error: "inkos.json read failed" }));
        expect(text).not.toContain(root);
      }
    }

    const root = await freshRoot(JSON.stringify({ name: "x", language: "zh", llm: { temperature: 0.3, stream: true } }));
    const app = createStudioServer({} as never, root);
    const saved = await app.request("/api/v1/project", json("PUT", { temperature: 0.7, stream: false, language: "en" }));
    expect(saved.status).toBe(200);
    expect(JSON.parse(await readFile(configPath(root), "utf-8"))).toMatchObject({
      name: "x", language: "en", llm: { temperature: 0.7, stream: false },
    });
    const language = await app.request("/api/v1/project/language", json("POST", { language: "zh" }));
    expect(language.status).toBe(200);
    expect(JSON.parse(await readFile(configPath(root), "utf-8")).language).toBe("zh");
  });

  it("数组根：`/project` 族 9 个写入点不再「回 200 却没存」", async () => {
    for (const [method, path, body] of PROJECT_WRITES) {
      const root = await freshRoot("[1]\n");
      const app = createStudioServer({} as never, root);

      const response = await app.request(path, json(method, body));
      expect(response.status, `${method} ${path}`).toBe(500);
      expect(await response.json(), `${method} ${path}`).toEqual(
        path === "/api/v1/project" || path === "/api/v1/project/language"
          ? { error: "inkos.json read failed" }
          : CONTRACT_NOT_OBJECT,
      );
      expect(await readFile(configPath(root), "utf-8"), `${method} ${path}`).toBe("[1]\n");
    }
  });

  // Windows 的目录只读位不拦新建文件，无法构造「读取成功、落盘失败」。
  it.skipIf(process.platform === "win32")("落盘失败必须报错，且不继续动 secrets.json", async () => {
    const root = await freshRoot(VALID_CONFIG);
    await mkdir(join(root, ".inkos"), { recursive: true });
    const secrets = JSON.stringify({ services: { "custom:X": { apiKey: testKey() } } });
    await writeFile(secretsPath(root), secrets);
    const app = createStudioServer({} as never, root);
    await chmod(root, 0o555);
    try {
      // 夹具自检：读取必须照常成功，否则测的不是「落盘失败」。
      const read = await app.request("/api/v1/services/config");
      expect(read.status).toBe(200);

      const response = await app.request("/api/v1/services/custom:X", json("DELETE"));
      expect(response.status).toBe(500);
      expect(await response.json()).toEqual({ error: "failed to save inkos.json" });
      expect(await readFile(configPath(root), "utf-8")).toBe(VALID_CONFIG);
      expect(await readFile(secretsPath(root), "utf-8")).toBe(secrets);
    } finally {
      await chmod(root, 0o755);
    }
  });

  it("inkos.json 的每个写入点都是原子替换写（文件身份更换、无临时文件残留）", async () => {
    for (const [method, path, body] of [
      ["PUT", "/api/v1/services/config", { defaultModel: "m1" }],
      ["DELETE", "/api/v1/services/custom:X", undefined],
      ["POST", "/api/v1/services/config/import-env", undefined],
      ["PUT", "/api/v1/cover/config", { service: "kkaiapi" }],
      ...PROJECT_WRITES,
    ] as Array<[string, string, unknown?]>) {
      const root = await freshRoot(VALID_CONFIG);
      await writeImportableEnv(root);
      const before = await stat(configPath(root));
      const app = createStudioServer({} as never, root);

      const response = await app.request(path, json(method, body));
      expect(response.status, `${method} ${path}: ${await response.clone().text()}`).toBe(200);

      // 就地重写保持 inode；「临时文件 + rename」必然换一个（旧 inode 在新文件
      // 创建之后才释放）。
      const after = await stat(configPath(root));
      expect(after.ino, `${method} ${path}`).not.toBe(before.ino);
      expect(JSON.parse(await readFile(configPath(root), "utf-8")).name, `${method} ${path}`).toBe("x");
      // 不留临时文件（`.inkos.json.tmp-*`）。
      const leftovers = (await readdir(root)).filter((name) => ![".env", ".inkos", "inkos.json"].includes(name));
      expect(leftovers, `${method} ${path}`).toEqual([]);
    }
  });
});
