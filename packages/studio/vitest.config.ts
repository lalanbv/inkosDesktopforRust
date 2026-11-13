import { defineConfig } from "vitest/config";
import { resolve } from "node:path";

export default defineConfig({
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
      // Subpath aliases must precede the bare package alias: a string alias also
      // matches `<key>/...`, so the bare entry would otherwise swallow these.
      // The app/build resolves these subpaths via core's package.json "exports"
      // (browser-safe, zod-only); tests resolve them to source like the bare pkg.
      "@actalk/inkos-core/interactive-film/evaluator": resolve(__dirname, "../core/src/interactive-film/evaluator.ts"),
      "@actalk/inkos-core/interactive-film/graph-schema": resolve(__dirname, "../core/src/interactive-film/graph-schema.ts"),
      "@actalk/inkos-core/forecast/schema": resolve(__dirname, "../core/src/forecast/schema.ts"),
      // 399 号：core exports 新增的三个子路径（hook-kind 由 R23 引入）。
      "@actalk/inkos-core/utils/hook-kind": resolve(__dirname, "../core/src/utils/hook-kind.ts"),
      "@actalk/inkos-core/utils/author-report": resolve(__dirname, "../core/src/utils/author-report.ts"),
      "@actalk/inkos-core/utils/author-error-catalog": resolve(__dirname, "../core/src/utils/author-error-catalog.ts"),
      "@actalk/inkos-core/llm/api-format": resolve(__dirname, "../core/src/llm/api-format.ts"),
      // 1008 号：models/book subpath（BookStatusSchema 枚举真源，草稿库校验用）
      "@actalk/inkos-core/models/book": resolve(__dirname, "../core/src/models/book.ts"),
      "@actalk/inkos-core": resolve(__dirname, "../core/src/index.ts"),
    },
  },
  test: {
    // .test.tsx：组件冒烟测试（react-dom/server renderToString，无需 DOM 环境）
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    fileParallelism: false,
    // server.ts is large enough that first-load esbuild transforms can exceed
    // Vitest's default 5s timeout on a cold full-suite run.
    testTimeout: 30_000,
    // 902 号实测采纳：threads 池省每文件 fork 开销，isolate:true 语义不变。
    // 2×全量 0 败 38.24s/38.21s vs forks 2×0 败 50.45s/48.55s（-21~24%）。
    // cli 面同款实验否决（6 败稳定复现=process.chdir worker 不支持，
    // 收益微 3.8→3.5s）——forks 留守，勿顺手改 cli。
    pool: "threads",
  },
});
