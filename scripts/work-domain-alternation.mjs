#!/usr/bin/env node
// work 域双引擎交替读写互证（884 号 M1b 验收——881 设计 §四.3「876 号盲区补法」）：
//   node scripts/work-domain-alternation.mjs [--keep]
//
// 同一项目根上 Node 引擎与 Rust 引擎交替读写 works/ 与 .inkos/harness.sqlite：
//   ① Node 引擎起 → GET works 域基线（Node 写读基线 + 夹具形状断言）
//   ② 换 Rust 引擎同根起 → GET 全域（Node 写的 work.json/sqlite 被 Rust 整份读活）
//   ③ Rust POST adopt → 200（源工件注册翻转 + 快照落位 + episode 事件序列）
//   ④ Node 侧文件级验证：core zod schema 解析 Rust 写的 work.json（写出字段集
//      与 TS strict 可读集精确互逆——876 号教训的活体闭环）+ sqlite 事件回读
//   ⑤ 换回 Node 引擎同根起 → GET /works/:id（Rust 写的状态被 TS 引擎活体读出
//      ——桌面回退场景的用户可见等价）
//
// 退出码非零 = 交替读写断裂（任一端拒读对端数据）。works 域零 LLM 依赖，无 mock。

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, writeFileSync, rmSync, openSync } from "node:fs";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { join, dirname, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { ensureRustBinFreshOrExit } from "./ensure-rust-bin.mjs";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const studioDir = join(repoRoot, "packages", "studio");
const rustBinary = process.env.INKOS_SMOKE_RUST_BIN
  ?? join(repoRoot, "engine-rs", "target", "debug", "inkos-engine-server");

const keep = process.argv.includes("--keep");
const fetchT = (input, init = {}) => fetch(input, { ...init, signal: AbortSignal.timeout(20_000) });

ensureRustBinFreshOrExit(repoRoot);
if (!existsSync(rustBinary)) {
  console.error(`[alt] ✗ Rust 二进制缺失（${rustBinary}）——先在 engine-rs 下 cargo build`);
  process.exit(1);
}

const children = [];
const startChild = (cmd, cmdArgs, opts, logPath) => {
  const out = openSync(logPath, "a");
  const child = spawn(cmd, cmdArgs, { ...opts, stdio: ["ignore", out, out], detached: true });
  children.push(child);
  return child;
};
const stopAll = () => {
  for (const child of children) {
    try { process.kill(-child.pid, "SIGKILL"); } catch { /* 已退出 */ }
  }
};
process.on("exit", () => stopAll());

const waitUntil = async (fn, timeoutMs, label) => {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try { if (await fn()) return true; } catch { /* not ready */ }
    await new Promise((r) => setTimeout(r, 400));
  }
  throw new Error(`等待超时：${label}`);
};

const fail = (message) => {
  console.error(`[alt] ✗ ${message}`);
  process.exitCode = 1;
};

// ── 夹具（walkthrough-fixture §2.9 同源形态；works 域 adopt 需 source/ 路径）──

const WORK_ID = "alt-work";
const REV1_TEXT = "# 章节 v1\n\n雨夜来客叩门。\n";
const REV2_TEXT = "# 章节 v2\n\n雨夜来客叩门；镜中人影与来客同貌。\n";
const sha256 = (text) => createHash("sha256").update(text, "utf-8").digest("hex");
const REV1_CHECKSUM = `sha256:${sha256(REV1_TEXT)}`;
const REV2_CHECKSUM = `sha256:${sha256(REV2_TEXT)}`;
const REV2_ID = `rev-${REV2_CHECKSUM.slice("sha256:".length, "sha256:".length + 16)}`;

const seedWorkEntity = async (root) => {
  const workDir = join(root, "works", WORK_ID);
  const revisionOf = (revisionId, text, status, parent, createdAt) => ({
    id: revisionId,
    parentRevisionId: parent,
    path: "source/chapter.md",
    snapshotPath: `revisions/chapter/${revisionId}.md`,
    contentType: "text/markdown",
    status,
    checksum: `sha256:${sha256(text)}`,
    byteLength: Buffer.byteLength(text, "utf-8"),
    createdAt,
  });
  const manifest = {
    version: 2,
    id: WORK_ID,
    title: "交替读写金标准",
    profileId: "workspace-default",
    language: "zh",
    status: "active",
    lineage: [],
    artifacts: [{
      id: "chapter",
      kind: "file",
      currentRevisionId: "r1",
      revisions: [
        revisionOf("r1", REV1_TEXT, "current", null, "2026-11-01T00:00:00.000Z"),
        revisionOf("r2", REV2_TEXT, "candidate", "r1", "2026-11-02T00:00:00.000Z"),
      ],
      metadata: { sourcePath: `works/${WORK_ID}/source/chapter.md`, kindSource: "storage-path-v1" },
    }],
    metadata: {},
    createdAt: "2026-11-01T00:00:00.000Z",
    updatedAt: "2026-11-02T00:00:00.000Z",
  };
  mkdirSync(join(workDir, "source"), { recursive: true });
  mkdirSync(join(workDir, "revisions", "chapter"), { recursive: true });
  writeFileSync(join(workDir, "source", "chapter.md"), REV1_TEXT);
  writeFileSync(join(workDir, "revisions", "chapter", "r1.md"), REV1_TEXT);
  writeFileSync(join(workDir, "revisions", "chapter", "r2.md"), REV2_TEXT);
  writeFileSync(join(workDir, "work.json"), `${JSON.stringify(manifest, null, 2)}\n`);

  // 项目技能种子（M2 documents PUT 写面载体）
  const skillDir = join(root, ".agents", "skills", "doc-skill");
  mkdirSync(join(skillDir, "docs"), { recursive: true });
  writeFileSync(
    join(skillDir, "SKILL.md"),
    "---\nname: doc-skill\ndescription: 交替互证种子技能\n---\n\n# doc-skill\n",
  );
  writeFileSync(join(skillDir, "docs", "notes.md"), "# 笔记\n");

  // episode 经 core dist CreativeEpisodeStore 播种（与 Node 回退端同一实现同一 schema）
  const storeModule = join(repoRoot, "packages", "core", "dist", "harness", "episode-store.js");
  if (!existsSync(storeModule)) {
    fail(`core dist 缺失（pnpm --filter @actalk/inkos-core build）——episode 播种跳过`);
    return;
  }
  const { CreativeEpisodeStore } = await import(pathToFileURL(storeModule).href);
  const store = new CreativeEpisodeStore(join(root, ".inkos", "harness.sqlite"));
  try {
    store.create({
      version: 2,
      id: "ep-alt-1",
      workId: WORK_ID,
      profileId: "workspace-default",
      status: "completed",
      startedAt: "2026-11-02T00:00:00.000Z",
      completedAt: "2026-11-02T00:01:00.000Z",
    });
    store.append({
      episodeId: "ep-alt-1",
      type: "stage.completed",
      workId: WORK_ID,
      capabilityId: "workspace",
      payload: { stage: "outline" },
    }, "2026-11-02T00:00:30.000Z");
  } finally {
    store.close();
  }
};

const getJson = async (port, path) => {
  const res = await fetchT(`http://127.0.0.1:${port}${path}`);
  let body = null;
  try { body = await res.json(); } catch { body = null; }
  return { status: res.status, body };
};

// ── 引擎编排 ─────────────────────────────────────────────────────────

const nodePort = 8912;
const rustPort = 8913;
const root = mkdtempSync(join(tmpdir(), "inkos-alt-"));
// 项目种子：Node 引擎启动门槛（inkos.json 必在；works 域无 LLM 依赖——
// services 指向死端口仅为过 schema）。
mkdirSync(join(root, ".inkos"), { recursive: true });
writeFileSync(
  join(root, "inkos.json"),
  JSON.stringify({ name: "inkos-work-alternation", version: "0.1.0", services: [{ service: "custom", name: "Dead", baseUrl: "http://127.0.0.1:9/v1" }] }),
);
writeFileSync(join(root, ".inkos", "secrets.json"), JSON.stringify({ services: { "custom:Dead": { apiKey: "unused" } } }));
console.log(`[alt] 项目根：${root}${keep ? "（--keep 保留）" : ""}`);

await seedWorkEntity(root);
if (process.exitCode === 1) {
  stopAll();
  if (!keep) rmSync(root, { recursive: true, force: true });
  process.exit(process.exitCode);
}

const startNodeEngine = () => {
  startChild(
    process.execPath,
    [join(studioDir, "node_modules", "tsx", "dist", "cli.mjs"), join(studioDir, "src", "api", "index.ts"), root],
    { cwd: studioDir, env: { ...process.env, INKOS_STUDIO_PORT: String(nodePort) } },
    join(root, "node-engine.log"),
  );
};
const startRustEngine = () => {
  startChild(
    rustBinary,
    [],
    {
      cwd: repoRoot,
      env: {
        ...process.env,
        INKOS_PORT: String(rustPort),
        INKOS_PROJECT_ROOT: root,
        INKOS_STATIC_DIR: join(studioDir, "dist"),
        // works 域零 LLM 依赖——指向死端口即可启动（写链不在本脚本路径上）。
        INKOS_LLM_BASE_URL: "http://127.0.0.1:9/v1",
        INKOS_BUILTIN_GENRES_DIR: join(repoRoot, "packages", "core", "genres"),
        INKOS_BUILTIN_SKILLS_DIR: join(repoRoot, "packages", "core", "skills"),
      },
    },
    join(root, "rust-engine.log"),
  );
};
const stopEngines = () => {
  for (const child of children.splice(0)) {
    try { process.kill(-child.pid, "SIGKILL"); } catch { /* 已退出 */ }
  }
  // 端口释放窗口
  return new Promise((r) => setTimeout(r, 800));
};

const checks = [];
const check = (label, ok, detail = "") => {
  checks.push({ label, ok });
  console.log(`[alt] ${ok ? "✓" : "✗"} ${label}${detail ? `｜${detail}` : ""}`);
  if (!ok) fail(label);
};

try {
  // ── ① Node 引擎：写读基线 ──
  startNodeEngine();
  await waitUntil(async () => (await fetchT(`http://127.0.0.1:${nodePort}/api/v1/works`)).ok, 30_000, "Node 引擎启动");
  {
    const { status, body } = await getJson(nodePort, "/api/v1/works");
    check("① Node /works 基线", status === 200 && body?.works?.length === 1 && body.works[0]?.artifactCount === 1, `status=${status}`);
    const detail = await getJson(nodePort, `/api/v1/works/${WORK_ID}`);
    check(
      "① Node /works/:id + episodes",
      detail.status === 200 && detail.body?.work?.id === WORK_ID && detail.body?.episodes?.length === 1
        && detail.body.episodes[0]?.id === "ep-alt-1",
    );
    const profiles = await getJson(nodePort, "/api/v1/profiles");
    check("① Node /profiles 内置 9", profiles.status === 200 && profiles.body?.profiles?.length === 9);
  }
  await stopEngines();

  // ── ② Rust 引擎同根读 Node 写的数据 ──
  startRustEngine();
  await waitUntil(async () => (await fetchT(`http://127.0.0.1:${rustPort}/api/v1/works`)).ok, 30_000, "Rust 引擎启动");
  {
    const { status, body } = await getJson(rustPort, "/api/v1/works");
    check(
      "② Rust 读 Node 写的 /works（交替读半环）",
      status === 200 && body?.works?.length === 1 && body.works[0]?.id === WORK_ID
        && body.works[0]?.artifactCount === 1
        && body.works[0]?.artifacts?.[0]?.currentRevisionId === "r1",
      `status=${status}`,
    );
    const detail = await getJson(rustPort, `/api/v1/works/${WORK_ID}`);
    check(
      "② Rust 读 Node 写的 episodes（harness.sqlite 交替读）",
      detail.status === 200 && detail.body?.episodes?.length === 1 && detail.body.episodes[0]?.id === "ep-alt-1",
    );
    const revision = await getJson(rustPort, `/api/v1/works/${WORK_ID}/artifacts/chapter/revisions/r2`);
    check(
      "② Rust 读 candidate 快照（checksum 面）",
      revision.status === 200 && revision.body?.content === REV2_TEXT,
    );
    const episodes = await getJson(rustPort, "/api/v1/episodes?workId=" + WORK_ID);
    check("② Rust /episodes 过滤读", episodes.status === 200 && episodes.body?.episodes?.length === 1);
    const profiles = await getJson(rustPort, "/api/v1/profiles");
    check("② Rust /profiles", profiles.status === 200 && profiles.body?.profiles?.length === 9);
  }

    // ── ③ Rust adopt（写面）──
    {
      const res = await fetchT(`http://127.0.0.1:${rustPort}/api/v1/works/${WORK_ID}/artifacts/chapter/revisions/r2/adopt`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ expectedCurrentRevisionId: "r1" }),
      });
      const body = await res.json().catch(() => null);
      check(
        "③ Rust adopt 200 + ActionResult 形状",
        res.status === 200 && body?.action?.status === "success"
          && body.action?.artifacts?.[0]?.revisionId === REV2_ID
          && body.action?.data?.kind === "work_revision_adopted"
          && body.action?.data?.previousRevisionId === "r1",
        `status=${res.status} body=${JSON.stringify(body)?.slice(0, 200)}`,
      );
      const detail = await getJson(rustPort, `/api/v1/works/${WORK_ID}`);
      check(
        "③ Rust 自读翻转态（r1→superseded + 新 current）",
        detail.status === 200 && detail.body?.work?.artifacts?.[0]?.currentRevisionId === REV2_ID,
      );
    }

    // ── ③b Rust 写面（885 号 M2）：profiles POST/PUT + documents PUT ──
    {
      const postProfile = await fetchT(`http://127.0.0.1:${rustPort}/api/v1/profiles`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          version: 2,
          id: "alt-custom",
          title: "交替互证自定义",
          description: "Rust 写面播种",
          capabilityIds: ["workspace"],
          requiredSkillIds: [],
          recommendedSkillIds: [],
          artifactKinds: [],
          confirmation: { inferredMutation: "execute", explicitRecoverableMutation: "execute", destructiveMutation: "confirm" },
          artifactSchemas: {},
          qualityCriteria: [],
          production: {},
        }),
      });
      const postBody = await postProfile.json().catch(() => null);
      check(
        "③b Rust POST /profiles 201（自定义落盘）",
        postProfile.status === 201 && postBody?.profile?.id === "alt-custom",
        `status=${postProfile.status}`,
      );
      const dupProfile = await fetchT(`http://127.0.0.1:${rustPort}/api/v1/profiles`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ...(await (await fetchT(`http://127.0.0.1:${rustPort}/api/v1/profiles`)).json()).profiles.find((p) => p.id === "alt-custom") }),
      });
      check("③b 重复 POST → 409 PROFILE_EXISTS", dupProfile.status === 409);

      const putProfile = await fetchT(`http://127.0.0.1:${rustPort}/api/v1/profiles/short-fiction`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          ...(await (await fetchT(`http://127.0.0.1:${rustPort}/api/v1/profiles`)).json()).profiles.find((p) => p.id === "short-fiction"),
          title: "短篇（交替互证覆盖）",
        }),
      });
      check("③b Rust PUT /profiles 内置覆盖 200", putProfile.status === 200);

      // documents PUT：先 GET 现文档，原样写回 + 新增一页（Rust 写面闭环）
      const docsBefore = await getJson(rustPort, "/api/v1/skills/doc-skill/documents").catch(() => ({ status: 404, body: null }));
      if (docsBefore.status === 200) {
        const documents = docsBefore.body.documents.map((doc) => ({ path: doc.path, content: doc.content }));
        documents.push({ path: "docs/alt-note.md", content: "# 交替互证追加\n" });
        const putDocs = await fetchT(`http://127.0.0.1:${rustPort}/api/v1/skills/doc-skill/documents`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ documents }),
        });
        const putBody = await putDocs.json().catch(() => null);
        check(
          "③b Rust PUT documents 200 + saved 面形状",
          putDocs.status === 200 && putBody?.id === "doc-skill" && Array.isArray(putBody?.saved)
            && putBody.saved.includes("docs/alt-note.md"),
          `status=${putDocs.status}`,
        );
      } else {
        check("③b Rust PUT documents（无 doc-skill 种子，跳过=信息行）", true, `GET=${docsBefore.status}`);
      }
    }
  await stopEngines();

  // ── ④ Node 文件级验证（Rust 写 → TS zod strict 读：876 号活体闭环）──
  {
    const coreDist = join(repoRoot, "packages", "core", "dist", "index.js");
    if (!existsSync(coreDist)) {
      fail("core dist 缺失（pnpm --filter @actalk/inkos-core build）——文件级验证跳过");
    } else {
      const core = await import(pathToFileURL(coreDist).href);
      const { readFileSync } = await import("node:fs");
      const raw = readFileSync(join(root, "works", WORK_ID, "work.json"), "utf-8");
      const manifest = core.WorkManifestSchema.parse(JSON.parse(raw));
      const artifact = manifest.artifacts[0];
      check(
        "④ TS zod strict 解析 Rust 写的 work.json（写出字段集精确互逆）",
        artifact.currentRevisionId === REV2_ID,
      );
      const superseded = artifact.revisions.find((revision) => revision.id === "r1");
      const adopted = artifact.revisions.find((revision) => revision.id === REV2_ID);
      check(
        "④ 修订翻转语义（r1 superseded / rev-<checksum16> current）",
        superseded?.status === "superseded" && adopted?.status === "current"
          && adopted?.checksum === REV2_CHECKSUM
          && adopted?.snapshotPath === `revisions/chapter/${REV2_ID}.md`,
      );
      const snapshotBytes = readFileSync(join(root, "works", WORK_ID, "revisions", "chapter", `${REV2_ID}.md`));
      check(
        "④ Rust 写的快照字节 = 候选内容（checksum 闭环）",
        `sha256:${createHash("sha256").update(snapshotBytes).digest("hex")}` === REV2_CHECKSUM,
      );
      const sourceBytes = readFileSync(join(root, "works", WORK_ID, "source", "chapter.md"));
      check("④ Rust 写的源文件 = 采纳内容", sourceBytes.toString("utf-8") === REV2_TEXT);
      const { CreativeEpisodeStore } = await import(pathToFileURL(join(repoRoot, "packages", "core", "dist", "harness", "episode-store.js")).href);
      const store = new CreativeEpisodeStore(join(root, ".inkos", "harness.sqlite"));
      try {
        const episodes = store.listEpisodes({ workId: WORK_ID, limit: 50 });
        const adoptedEpisode = episodes.find((episode) => episode.id !== "ep-alt-1");
        check(
          "④ TS 读 Rust 写的 adopt episode（completed）",
          adoptedEpisode?.status === "completed" && adoptedEpisode?.profileId === "workspace-default",
          `episodes=${episodes.length}`,
        );
        if (adoptedEpisode) {
          const events = store.listEvents(adoptedEpisode.id);
          const types = events.map((event) => event.type);
          check(
            "④ 事件序列完整（started→action-started→synced→completed→episode-completed）",
            JSON.stringify(types) === JSON.stringify([
              "episode-started", "action-started", "work-artifacts-synced", "action-completed", "episode-completed",
            ]),
            types.join("→"),
          );
        }
      } finally {
        store.close();
      }
      // M2 写面文件级验证：TS zod 解析 Rust 写的 profile 文件（strict 字段集互逆）
      const customRaw = readFileSync(join(root, ".inkos", "profiles", "alt-custom.json"), "utf-8");
      const customParsed = core.WorkProfileSchema.parse(JSON.parse(customRaw));
      check(
        "④ TS zod strict 解析 Rust 写的自定义 profile",
        customParsed.id === "alt-custom" && customParsed.title === "交替互证自定义",
      );
      const overriddenRaw = readFileSync(join(root, ".inkos", "profiles", "short-fiction.json"), "utf-8");
      const overriddenParsed = core.WorkProfileSchema.parse(JSON.parse(overriddenRaw));
      check(
        "④ TS zod strict 解析 Rust 写的内置覆盖 profile（contextRecipe 保持）",
        overriddenParsed.title === "短篇（交替互证覆盖）"
          && overriddenParsed.contextRecipe?.id === "short-fiction"
          && overriddenParsed.artifactSchemas?.["source/final/short-story.json"] === "short-manuscript",
      );
      const noteBytes = readFileSync(join(root, ".agents", "skills", "doc-skill", "docs", "alt-note.md"), "utf-8");
      check("④ Rust 写的文档追加页在位", noteBytes === "# 交替互证追加\n");
    }
  }

  // ── ⑤ Node 引擎同根读 Rust 写的状态（用户可见回退等价）──
  startNodeEngine();
  await waitUntil(async () => (await fetchT(`http://127.0.0.1:${nodePort}/api/v1/works`)).ok, 30_000, "Node 引擎二次启动");
  {
    const detail = await getJson(nodePort, `/api/v1/works/${WORK_ID}`);
    check(
      "⑤ Node 引擎读 Rust adopt 后的状态（交替读写全环）",
      detail.status === 200 && detail.body?.work?.artifacts?.[0]?.currentRevisionId === REV2_ID,
    );
    const revision = await getJson(nodePort, `/api/v1/works/${WORK_ID}/artifacts/chapter/revisions/${REV2_ID}`);
    check("⑤ Node 读 Rust 写的快照", revision.status === 200 && revision.body?.content === REV2_TEXT);
    // M2 写面用户可见等价：Node 引擎归并出 Rust 写的自定义/覆盖 profile 与追加文档
    const profiles = await getJson(nodePort, "/api/v1/profiles");
    const served = profiles.body?.profiles ?? [];
    const custom = served.find((profile) => profile.id === "alt-custom");
    const overridden = served.find((profile) => profile.id === "short-fiction");
    check(
      "⑤ Node 读 Rust 写的 profiles（自定义在列+内置覆盖生效）",
      profiles.status === 200 && served.length === 10
        && custom?.title === "交替互证自定义"
        && overridden?.title === "短篇（交替互证覆盖）",
      `profiles=${served.length}`,
    );
    const documents = await getJson(nodePort, "/api/v1/skills/doc-skill/documents");
    check(
      "⑤ Node 读 Rust 写的文档追加页",
      documents.status === 200
        && (documents.body?.documents ?? []).some((doc) => doc.path === "docs/alt-note.md"),
    );
  }
} finally {
  await stopEngines();
  if (!keep) {
    rmSync(root, { recursive: true, force: true });
    console.log("[alt] 临时根已清理（--keep 可保留现场）");
  } else {
    console.log(`[alt] 现场保留：${root}`);
  }
}

const failed = checks.filter((entry) => !entry.ok);
console.log(`[alt] ═══ 交替读写互证：${checks.length - failed.length}/${checks.length} 通过 ═══`);
if (failed.length) {
  for (const entry of failed) console.error(`[alt] ✗ ${entry.label}`);
  process.exit(1);
}
console.log("[alt] ✓ 双引擎交替读写互证全环通过（Node 写→Rust 读→Rust adopt→TS strict 读→Node 读）");
