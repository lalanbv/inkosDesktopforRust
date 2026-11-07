import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // 只收 src 下源码测试：vitest 5 不再把 dist 排除出默认收集面，
    // 构建产物里的编译副本（dist/__tests__/*.test.js）会被重复执行。
    include: ["src/__tests__/**/*.test.ts"],
    // CLI integration tests spawn real child processes; the default 5s timeout
    // is too aggressive when the whole workspace runs in parallel.
    testTimeout: 60_000,
    // 904 号实测采纳（与 core/studio 配置面统一）：threads 池省每文件
    // fork 开销，isolate:true 语义不变。2×全量 0 败 3.87s/3.90s vs forks
    // 4.10s（收益微 ~-5%，稳定为正）。902 否决的阻断站 process.chdir
    // 已收（tui-command.test.ts 改 mock cwd 读数，worker 下合法）。
    pool: "threads",
  },
});
