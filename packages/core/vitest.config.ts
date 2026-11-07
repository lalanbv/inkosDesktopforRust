import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/__tests__/**/*.test.ts"],
    // Flow tests perform durable filesystem transactions. Bound concurrent
    // workers so disk contention does not consume their individual time budgets.
    maxWorkers: 2,
    // Some pipeline-runner tests can approach Vitest's default 5s timeout
    // under full parallel runs; keep this high enough to avoid false kills.
    testTimeout: 30_000,
    // 901 号实测采纳：threads 池省每文件 fork 开销，isolate:true 语义不变
    // （每文件独立模块注册表，vi.mock/env/server 句柄隔离面与 forks 同级）。
    // 2×全量 0 败 48.11s/50.34s vs forks 2×0 败 56.42s/57.57s（-13~16%）。
    // isolate:false 同轮实测否决：47→67 败级联放大（40.5s→143.5s），
    // 且 mock 泄漏致真实外网调用（llm-retry 败面真实 fetch 打外部端点）。
    pool: "threads",
  },
});
