import { readdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { commitAtomicFileSet, type AtomicFileWrite } from "../utils/atomic-file-set.js";

export type ProductionKind =
  | "long-fiction"
  | "short-fiction"
  | "script"
  | "storyboard"
  | "interactive-film"
  | "play"
  | "translation";

export type ProductionRunStatus =
  | "pending"
  | "running"
  | "needs-review"
  | "complete"
  | "failed"
  | "cancelled";

export type ProductionObservationSeverity = "info" | "warning" | "blocking";

export interface ProductionObservation {
  readonly metric: string;
  readonly expected: unknown;
  readonly actual: unknown;
  readonly severity: ProductionObservationSeverity;
  readonly evidence?: string;
  readonly repairable: boolean;
}

export interface ProductionRunSnapshot {
  readonly version: 1;
  readonly kind: ProductionKind;
  readonly id: string;
  readonly status: ProductionRunStatus;
  readonly stage: string;
  readonly artifacts: ReadonlyArray<string>;
  readonly observations: ReadonlyArray<ProductionObservation>;
  readonly model?: string;
  readonly skillIds?: ReadonlyArray<string>;
  readonly resumeCursor?: string;
  readonly error?: string;
  readonly updatedAt: string;
}

export function createProductionRunSnapshot(
  input: Omit<ProductionRunSnapshot, "version" | "updatedAt"> & { readonly updatedAt?: string },
): ProductionRunSnapshot {
  return {
    version: 1,
    ...input,
    updatedAt: input.updatedAt ?? new Date().toISOString(),
  };
}

export function createRangeObservation(input: {
  readonly metric: string;
  readonly actual: number;
  readonly target: number;
  readonly min: number;
  readonly max: number;
  readonly unit: string;
  readonly evidence?: string;
  readonly hard?: boolean;
}): ProductionObservation {
  const inRange = input.actual >= input.min && input.actual <= input.max;
  return {
    metric: input.metric,
    expected: {
      target: input.target,
      min: input.min,
      max: input.max,
      unit: input.unit,
    },
    actual: { value: input.actual, unit: input.unit },
    severity: inRange ? "info" : input.hard === false ? "warning" : "blocking",
    ...(input.evidence ? { evidence: input.evidence } : {}),
    repairable: !inRange,
  };
}

/**
 * Commit validated artifacts and publish the run snapshot last. The snapshot is
 * operational truth: a completed run can never point at a half-written set.
 */
export async function commitProductionArtifacts(input: {
  readonly rootDir: string;
  readonly artifacts: ReadonlyArray<AtomicFileWrite>;
  readonly runPath: string;
  readonly run: ProductionRunSnapshot;
  readonly deletes?: ReadonlyArray<string>;
  readonly validate?: () => void | Promise<void>;
}): Promise<void> {
  await input.validate?.();
  await commitAtomicFileSet({
    rootDir: input.rootDir,
    writes: [
      ...input.artifacts,
      {
        relativePath: input.runPath,
        content: `${JSON.stringify(input.run, null, 2)}\n`,
      },
    ],
    deletes: input.deletes,
  });
}

export async function writeProductionRunSnapshot(input: {
  readonly rootDir: string;
  readonly runPath: string;
  readonly run: ProductionRunSnapshot;
}): Promise<void> {
  await commitProductionArtifacts({
    rootDir: input.rootDir,
    artifacts: [],
    runPath: input.runPath,
    run: input.run,
  });
}

/**
 * 运行时观测工件的保留章数（每书）。`INKOS_RUNTIME_RETENTION_CHAPTERS`
 * 可覆盖；0 = 关闭清理。与 Rust production::runtime_retention_chapters 对齐。
 */
export function runtimeRetentionChapters(): number {
  const raw = process.env.INKOS_RUNTIME_RETENTION_CHAPTERS;
  const parsed = raw === undefined ? Number.NaN : Number(raw);
  return Number.isFinite(parsed) && parsed >= 0 ? Math.floor(parsed) : 20;
}

/**
 * 清理旧章的运行时观测工件（200 号稳定性审计，与 Rust
 * production::prune_runtime_artifacts 对齐）：`story/runtime/` 下每章累积
 * run/trace/context/rule-stack 四类观测文件，只保留最近 `keep` 章的；
 * plan.md / intent.md 治理产物保留。逐文件失败忽略——事后打扫不报错。
 */
export async function pruneRuntimeArtifacts(
  bookDir: string,
  latestChapter: number,
  keep: number,
): Promise<void> {
  if (keep <= 0 || latestChapter <= keep) return;
  const cutoff = latestChapter - keep;
  const runtimeDir = join(bookDir, "story", "runtime");
  let entries: string[];
  try {
    entries = await readdir(runtimeDir);
  } catch {
    return;
  }
  const observabilitySuffixes = ["run.json", "trace.json", "context.json", "rule-stack.yaml"];
  for (const name of entries) {
    if (!observabilitySuffixes.some((suffix) => name.endsWith(suffix))) continue;
    const digits = name.startsWith("chapter-") ? name.slice("chapter-".length).split(".")[0] : undefined;
    const number = digits === undefined ? Number.NaN : Number(digits);
    if (Number.isInteger(number) && number > 0 && number <= cutoff) {
      await rm(join(runtimeDir, name), { force: true }).catch(() => undefined);
    }
  }
}
