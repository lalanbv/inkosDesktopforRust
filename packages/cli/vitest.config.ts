import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // 只收 src 下源码测试：vitest 5 不再把 dist 排除出默认收集面，
    // 构建产物里的编译副本（dist/__tests__/*.test.js）会被重复执行。
    include: ["src/__tests__/**/*.test.ts"],
    // CLI integration tests spawn real child processes; the default 5s timeout
    // is too aggressive when the whole workspace runs in parallel.
    testTimeout: 60_000,
  },
});
