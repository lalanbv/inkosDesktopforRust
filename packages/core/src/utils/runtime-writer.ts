import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { commitAtomicFileSet } from "./atomic-file-set.js";
import type {
  ChapterTrace,
  ContextPackage,
} from "../models/input-governance.js";

export interface RuntimeArtifactWriteResult {
  readonly contextPath: string;
  readonly tracePath: string;
}

export async function writeGovernedRuntimeArtifacts(params: {
  readonly runtimeDir: string;
  readonly chapterNumber: number;
  readonly contextPackage: ContextPackage;
  readonly trace: ChapterTrace;
}): Promise<RuntimeArtifactWriteResult> {
  await mkdir(params.runtimeDir, { recursive: true });

  const chapterSlug = `chapter-${String(params.chapterNumber).padStart(4, "0")}`;
  const contextPath = join(params.runtimeDir, `${chapterSlug}.context.json`);
  const tracePath = join(params.runtimeDir, `${chapterSlug}.trace.json`);

  // 780 号：双件是 compose → write 的交接组，事务集提交——崩溃收敛到
  // 「两件要么全旧/全缺、要么全新」（镜像 Rust 779 号三件套同款升级）。
  await commitAtomicFileSet({
    rootDir: params.runtimeDir,
    writes: [
      { relativePath: `${chapterSlug}.context.json`, content: JSON.stringify(params.contextPackage, null, 2) },
      { relativePath: `${chapterSlug}.trace.json`, content: JSON.stringify(params.trace, null, 2) },
    ],
  });

  return {
    contextPath,
    tracePath,
  };
}
