#!/usr/bin/env node
// 对跑支撑脚本：用 2.0 TS 腿**自己的写入面**（core 的 PlayStore / PlayDB）在项目
// 根里原生创建 / 追加一个互动叙事世界——works/<id>/source 布局。与 TS 腿走的是
// 同一份写入代码，不是手搓的夹具（手搓=第二真源，schema 一变就是假红）。
//
//   node seedPlayWorld.mjs create      <projectRoot> <worldId>
//   node seedPlayWorld.mjs append-turn <projectRoot> <worldId> [content]
//
// `create` = 建世界 + run `main`（play.db + 一轮转录 + 当前状态）。
// 直接导入 core 构建产物（与 scripts/migrate-legacy-works.mjs 同法）；缺失时给出
// 可行动指引并非零退出。
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..", "..", "..");
const [command, root, worldId, content] = process.argv.slice(2);

if (!command || !root || !worldId) {
  console.error("用法：node seedPlayWorld.mjs <create|append-turn> <projectRoot> <worldId> [content]");
  process.exit(2);
}

// 回环/本地文件场景不需要代理（与 migrate-legacy-works.mjs 同款豁免）。
process.env.NO_PROXY = [process.env.NO_PROXY, "127.0.0.1", "localhost"].filter(Boolean).join(",");
process.env.no_proxy = process.env.NO_PROXY;

const core = await import(pathToFileURL(join(repoRoot, "packages", "core", "dist", "index.js")).href)
  .catch((error) => {
    console.error(`无法加载 packages/core/dist（先执行 pnpm --filter @actalk/inkos-core build）：${error.message}`);
    process.exit(3);
  });

const store = new core.PlayStore(root);
if (command === "create") {
  const world = await store.createWorld({
    id: worldId,
    title: "TS 腿世界",
    premise: "雾港的渡轮只在午夜靠岸。",
    worldContract: "",
    visualContract: "",
    mode: "open",
    language: "zh",
  });
  await store.ensureRun(world.id, "main");
  const db = core.createPlayDB(store.runDir(world.id, "main"));
  // 图谱里放一个实体：产品引擎要能从 Node 腿建的 play.db 里读出它。
  db.upsertEntity({ id: "ts-hero", type: "actor", label: "雾港来客", summary: "" });
  db.close?.();
  await store.appendTranscriptTurn(world.id, "main", {
    role: "user",
    content: "TS 腿第一问",
    timestamp: Date.now(),
  });
  await store.saveCurrentState(world.id, "main", { turn: 1, lastSummary: "TS 腿第一问" });
  console.log(`[seed-play-world] created works/${world.id}/source runs/main`);
} else if (command === "append-turn") {
  await store.appendTranscriptTurn(worldId, "main", {
    role: "assistant",
    content: content ?? "TS 腿第二答",
    timestamp: Date.now(),
  });
  console.log(`[seed-play-world] appended turn to works/${worldId}/source/runs/main`);
} else {
  console.error(`未知命令：${command}`);
  process.exit(2);
}
