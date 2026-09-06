import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  commitProductionArtifacts,
  createProductionRunSnapshot,
  createRangeObservation,
  pruneRuntimeArtifacts,
  runtimeRetentionChapters,
} from "../production/harness.js";

describe("production harness", () => {
  const roots: string[] = [];

  afterEach(async () => {
    await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
  });

  it("commits artifacts before the authoritative completion snapshot", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-production-"));
    roots.push(root);
    const run = createProductionRunSnapshot({
      kind: "script",
      id: "night-shift",
      status: "complete",
      stage: "commit",
      artifacts: ["script.md"],
      observations: [],
    });

    await commitProductionArtifacts({
      rootDir: root,
      artifacts: [{ relativePath: "script.md", content: "# Night Shift" }],
      runPath: "status.json",
      run,
      validate: () => {
        expect(run.artifacts).toEqual(["script.md"]);
      },
    });

    await expect(readFile(join(root, "script.md"), "utf-8")).resolves.toBe("# Night Shift");
    await expect(readFile(join(root, "status.json"), "utf-8")).resolves.toContain('"status": "complete"');
  });

  it("expresses measurable failures without guessing creative intent", () => {
    expect(createRangeObservation({
      metric: "chapter-length",
      actual: 730,
      target: 1000,
      min: 900,
      max: 1200,
      unit: "zh_chars",
      evidence: "chapter 4",
    })).toMatchObject({
      severity: "blocking",
      repairable: true,
      actual: { value: 730, unit: "zh_chars" },
    });
  });
});

describe("pruneRuntimeArtifacts（200 号运行时观测工件保留策略）", () => {
  it("只保留最近 keep 章的观测工件，治理产物 plan/intent 保留", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-prune-"));
    try {
      const runtime = join(root, "story", "runtime");
      await mkdir(runtime, { recursive: true });
      for (let n = 1; n <= 5; n += 1) {
        const padded = String(n).padStart(4, "0");
        for (const suffix of ["run.json", "trace.json", "context.json", "rule-stack.yaml"]) {
          await writeFile(join(runtime, `chapter-${padded}.${suffix}`), "x", "utf-8");
        }
        await writeFile(join(runtime, `chapter-${padded}.plan.md`), "plan", "utf-8");
        await writeFile(join(runtime, `chapter-${padded}.intent.md`), "intent", "utf-8");
      }

      await pruneRuntimeArtifacts(root, 5, 2);

      const remaining = await readdir(runtime);
      for (const n of [1, 2, 3]) {
        const padded = String(n).padStart(4, "0");
        expect(remaining).not.toContain(`chapter-${padded}.run.json`);
        expect(remaining).not.toContain(`chapter-${padded}.trace.json`);
      }
      for (const n of [4, 5]) {
        const padded = String(n).padStart(4, "0");
        expect(remaining).toContain(`chapter-${padded}.run.json`);
        expect(remaining).toContain(`chapter-${padded}.trace.json`);
      }
      // 治理产物全保留。
      for (let n = 1; n <= 5; n += 1) {
        const padded = String(n).padStart(4, "0");
        expect(remaining).toContain(`chapter-${padded}.plan.md`);
        expect(remaining).toContain(`chapter-${padded}.intent.md`);
      }
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it("keep=0 或未超出保留窗口时为无操作；默认保留章数为 20", async () => {
    const root = await mkdtemp(join(tmpdir(), "inkos-prune-noop-"));
    try {
      const runtime = join(root, "story", "runtime");
      await mkdir(runtime, { recursive: true });
      await writeFile(join(runtime, "chapter-0001.run.json"), "x", "utf-8");
      await pruneRuntimeArtifacts(root, 5, 0);
      expect((await readdir(runtime)).join(",")).toContain("chapter-0001.run.json");
      await pruneRuntimeArtifacts(root, 1, 20);
      expect((await readdir(runtime)).join(",")).toContain("chapter-0001.run.json");
      const previousEnv = process.env.INKOS_RUNTIME_RETENTION_CHAPTERS;
      delete process.env.INKOS_RUNTIME_RETENTION_CHAPTERS;
      try {
        expect(runtimeRetentionChapters()).toBe(20);
      } finally {
        if (previousEnv !== undefined) process.env.INKOS_RUNTIME_RETENTION_CHAPTERS = previousEnv;
      }
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
