#!/usr/bin/env node
// 旧书迁移独立入口：`books/<id>`（及 shorts/dramas/… 各 legacy 目录）→
// `works/<id>/source` + `work.json`。与 CLI `inkos work migrate`
// （packages/cli/src/commands/work.ts）调用同一个 core 函数——2.0 的 `/books`
// 读面走 works 清单，legacy `books/` 目录不经迁移对 TS 腿不可见。
//
//   node scripts/migrate-legacy-works.mjs <projectRoot>
//
// 用途：双引擎夹具的「双根布局」播种（engine-rs 对跑 `write_fixture`——同一份
// 书既要让 fork 引擎在 `books/` 下读到，也要让 2.0 TS 腿在 `works/` 下读到；
// 原目录保留，两棵树内容一致）。scripts/ 不在 workspace 依赖链上，直接导入
// core 构建产物；缺失时给出可行动指引并非零退出。
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..");
const root = process.argv[2];

if (!root) {
  console.error("用法：node scripts/migrate-legacy-works.mjs <projectRoot>");
  process.exit(2);
}

// 回环/本地文件场景不需要代理；与 walkthrough-fixture 同款豁免，免得系统代理
// 干扰 core 加载期的任何本机请求。
process.env.NO_PROXY = [process.env.NO_PROXY, "127.0.0.1", "localhost"].filter(Boolean).join(",");
process.env.no_proxy = process.env.NO_PROXY;

const core = await import(pathToFileURL(join(repoRoot, "packages", "core", "dist", "index.js")).href)
  .catch((error) => {
    console.error(`无法加载 packages/core/dist（先执行 pnpm --filter @actalk/inkos-core build）：${error.message}`);
    process.exit(3);
  });

const results = await core.migrateLegacyWorks(root, { apply: true });
for (const item of results) {
  console.log(`[migrate-legacy-works] ${item.source} | ${item.status}${item.error ? ` | ${item.error}` : ""}`);
}
// 「已迁移 / 无可迁移」都算成功（幂等重跑）；invalid / conflict 才是失败。
const failed = results.some((item) => item.status === "invalid" || item.status === "conflict");
process.exit(failed ? 1 : 0);
