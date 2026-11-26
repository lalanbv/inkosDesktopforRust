// R62/1690 号：属性测试语料生成器（一次性可复跑——seed 固定可复现）。
// 产 8 组语料进 packages/studio/src/__tests__/golden/property-corpus/*.json，
// 每文件 {seed, generatedAt, sampleCount, samples:[...]}（期望值由当前实现
// 现场计算——回放测试双端逐样本断言；实现变更时重跑本脚本换基线，
// diff 即「行为有意变更」的审查面）。
// 用法：node scripts/generate-property-corpus.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { createRequire } from "node:module";
import * as core from "../packages/core/dist/index.js";
import * as exportArtifact from "../packages/core/dist/interaction/export-artifact.js";
import * as writingGoal from "../packages/core/dist/utils/writing-goal.js";
import * as usageBudget from "../packages/core/dist/utils/usage-budget.js";
// fast-check 只在 packages/core 的 devDependencies（pnpm 隔离）——从 core 包根
// createRequire 解析；fc4 的 ESM 直引在裸 Node 下 string() 内部初始化缺陷（实测）。
const require = createRequire(new URL("../packages/core/package.json", import.meta.url));
const fc = require("fast-check");
const fn = {
  docxEscapeXml: exportArtifact.docxEscapeXml,
  mdZipEntryPath: exportArtifact.mdZipEntryPath,
  dailyWordDelta: writingGoal.dailyWordDelta,
  goalStreak: writingGoal.goalStreak,
  expectedFinish: writingGoal.expectedFinish,
  shiftDayKey: writingGoal.shiftDayKey,
  usageProjection: usageBudget.usageProjection,
  structureInvariant: core.structureInvariant,
};

const SEED = 20261126;
const COUNT = 200;
const generatedAt = "2026-11-26T00:00:00.000Z";
const outDir = join(dirname(new URL(import.meta.url).pathname.replace(/^\/private/, "")), "..", "packages", "studio", "src", "__tests__", "golden", "property-corpus");
fc.configureGlobal({ seed: SEED });

const pad = (n, width) => String(n).padStart(width, "0");
const hexFp = fc.integer({ min: 0, max: 0xffffffff }).map((value) => value.toString(16).padStart(8, "0"));
const dayKeys = ["2026-11-20", "2026-11-21", "2026-11-22", "2026-11-23", "2026-11-24", "2026-11-25", "2026-11-26"];
const dayKeyArb = fc.constantFrom(...dayKeys);
const serviceKeys = fc.constantFrom("openai", "anthropic", "custom-x", "kkaiapi");

const files = {};

// ① docxEscapeXml：任意字符串 → 转义输出。
files["docx-escape-xml"] = fc
  .sample(fc.string({ maxLength: 120 }), COUNT)
  .map((input) => ({ input, output: fn.docxEscapeXml(input) }));

// ② mdZipEntryPath：卷可选 + 章号/标题 → 规范 zip 路径。
files["md-zip-entry-path"] = fc
  .sample(
    fc.record({
      volumeId: fc.option(fc.string({ minLength: 1, maxLength: 12 }), { nil: undefined }),
      volumes: fc.array(
        fc.record({ id: fc.string({ minLength: 1, maxLength: 12 }), title: fc.string({ maxLength: 16 }), order: fc.integer({ min: 1, max: 99 }) }),
        { maxLength: 4 },
      ),
      chapterNumber: fc.integer({ min: 1, max: 9999 }),
      chapterTitle: fc.string({ maxLength: 24 }),
    }),
    COUNT,
  )
  .map((input) => ({ input, output: fn.mdZipEntryPath(input.volumeId, input.volumes, input.chapterNumber, input.chapterTitle) }));

// ③ dailyWordDelta：写作日志行（ts 前 10 位为 dayKey 的固定口径）→ 按日聚合。
files["daily-word-delta"] = fc
  .sample(
    fc.array(
      fc.record({
        ts: fc.record({ day: dayKeyArb, hour: fc.integer({ min: 0, max: 23 }) }).map(({ day, hour }) => `${day}T${pad(hour, 2)}:00:00.000Z`),
        chapterNum: fc.integer({ min: 1, max: 99 }),
        before: fc.integer({ min: 0, max: 50000 }),
        after: fc.integer({ min: 0, max: 50000 }),
      }),
      { maxLength: 20 },
    ),
    COUNT,
  )
  .map((rows) => ({ rows, output: fn.dailyWordDelta(rows, (row) => row.ts.slice(0, 10)) }));

// ④ goalStreak：日差数组 + 目标 + 今天键 → 连续达成天数。
files["goal-streak"] = fc
  .sample(
    fc.record({
      deltas: fc.array(fc.record({ dayKey: dayKeyArb, delta: fc.integer({ min: -500, max: 3000 }) }), { maxLength: 12 }),
      dailyGoalWords: fc.integer({ min: 1, max: 2000 }),
      todayKey: dayKeyArb,
    }),
    COUNT,
  )
  .map((input) => ({ input, output: fn.goalStreak(input.deltas, input.dailyGoalWords, input.todayKey) }));

// ⑤ usageProjection：用量行 → 按服务聚合（daily/monthly/total；回放按服务键排序比对）。
files["usage-projection"] = fc
  .sample(
    fc.record({
      rows: fc.array(
        fc.record({
          serviceKey: serviceKeys,
          model: fc.constantFrom("m1", "m2"),
          promptTokens: fc.integer({ min: 0, max: 50000 }),
          completionTokens: fc.integer({ min: 0, max: 50000 }),
          totalTokens: fc.integer({ min: 0, max: 100000 }),
          ts: fc.record({ day: dayKeyArb, hour: fc.integer({ min: 0, max: 23 }) }).map(({ day, hour }) => `${day}T${pad(hour, 2)}:00:00.000Z`),
        }),
        { maxLength: 24 },
      ),
      todayKey: dayKeyArb,
    }),
    COUNT,
  )
  .map(({ rows, todayKey }) => {
    const projection = fn.usageProjection(rows, (row) => row.ts.slice(0, 10), todayKey);
    const output = [...projection.entries()]
      .map(([serviceKey, usage]) => ({ serviceKey, ...usage }))
      .sort((a, b) => (a.serviceKey < b.serviceKey ? -1 : 1));
    return { rows, todayKey, output };
  });

// ⑥ expectedFinish：当前/目标/日节奏 + 今天键 → 完成日或 undefined。
files["expected-finish"] = fc
  .sample(
    fc.record({
      currentWords: fc.integer({ min: 0, max: 100000 }),
      targetWords: fc.integer({ min: 0, max: 100000 }),
      dailyPaceWords: fc.integer({ min: -10, max: 5000 }),
      todayKey: dayKeyArb,
    }),
    COUNT,
  )
  .map((input) => ({ input, output: fn.expectedFinish(input.currentWords, input.targetWords, input.dailyPaceWords, input.todayKey) ?? null }));

// ⑦ shiftDayKey：合法 dayKey ± 天数（含跨月跨年大步长）。
files["shift-day-key"] = fc
  .sample(
    fc.record({
      dayKey: fc.record({ year: fc.integer({ min: 2000, max: 2030 }), month: fc.integer({ min: 1, max: 12 }), day: fc.integer({ min: 1, max: 28 }) })
        .map(({ year, month, day }) => `${year}-${pad(month, 2)}-${pad(day, 2)}`),
      days: fc.integer({ min: -800, max: 800 }),
    }),
    COUNT,
  )
  .map((input) => ({ input, output: fn.shiftDayKey(input.dayKey, input.days) }));

// ⑧ structureInvariant：随机 before 清单 → 按 op 构造 after（重排/拆分/合并）
//    + 20% 概率注入破坏（丢成员/改指纹）→ 判定结果与违反项逐字。
const fingerprint = fc.record({ hex: hexFp, salt: fc.integer({ min: 0, max: 999999 }) })
  .map(({ hex, salt }) => `fp-${hex}-${salt}`);
const chapterList = fc.array(fc.record({ fingerprint }), { minLength: 1, maxLength: 8 }).map((items) =>
  items.map((item, index) => ({
    number: index + 1,
    fileName: `${pad(index + 1, 4)}_章${index + 1}.md`,
    fingerprint: `${item.fingerprint}-${index + 1}`,
  })),
);
const opArb = fc.constantFrom("reorder", "split", "merge");
function buildAfter(op, before, mutate) {
  const at = Math.min(before.length, Math.max(1, Math.floor(before.length / 2) + 1));
  let after;
  if (op === "reorder") {
    after = [...before].reverse();
  } else if (op === "split") {
    const novel = `fp-novel-${at + 1}-${before.length}`;
    after = [];
    for (const item of before) {
      if (item.number < at) after.push(item);
      else if (item.number === at) {
        after.push({ ...item, fingerprint: `fp-head-${item.number}-${before.length}` });
        after.push({ number: item.number + 1, fileName: `${pad(item.number + 1, 4)}_新.md`, fingerprint: novel });
      } else {
        after.push({ ...item, number: item.number + 1, fileName: `${pad(item.number + 1, 4)}_${item.fileName.split("_")[1] ?? "章.md"}` });
      }
    }
  } else {
    const removed = Math.min(before.length, 2);
    after = [];
    for (const item of before) {
      if (item.number === removed) continue;
      const target = item.number > removed ? item.number - 1 : item.number;
      const merged = target === removed - 1;
      after.push({ ...item, number: target, fileName: merged ? `${pad(target, 4)}_合并.md` : item.fileName, fingerprint: merged ? `fp-merged-${before.length}` : item.fingerprint });
    }
  }
  if (mutate === "drop" && after.length > 0) {
    after = after.slice(1);
  }
  if (mutate === "fingerprint") {
    // 注入选「指纹在 before 多重集中」的项（移位章）——keep/被删章的指纹变化
    // 是允许语义（合并覆写/章被删），注入它们不可观测。
    const beforeFps = new Set(before.map((item) => item.fingerprint));
    for (let index = after.length - 1; index >= 0; index -= 1) {
      if (beforeFps.has(after[index].fingerprint)) {
        after[index] = { ...after[index], fingerprint: `${after[index].fingerprint}-mutated` };
        break;
      }
    }
  }
  return after;
}
files["structure-invariant"] = fc
  .sample(
    fc.record({
      before: chapterList,
      op: opArb,
      mutate: fc.constantFrom("none", "none", "none", "drop", "fingerprint"),
    }),
    COUNT,
  )
  .map(({ before, op, mutate }) => {
    const structureOp = op === "split" ? { kind: "split", at: Math.min(before.length, Math.max(1, Math.floor(before.length / 2) + 1)) } : op === "merge" ? { kind: "merge", removed: Math.min(before.length, 2) } : { kind: "reorder" };
    const after = buildAfter(op, before, mutate);
    const { violations } = fn.structureInvariant(structureOp, before, after);
    return { op: structureOp, before, after, ok: violations.length === 0, violations };
  });

mkdirSync(outDir, { recursive: true });
let total = 0;
for (const [name, samples] of Object.entries(files)) {
  if (samples.length !== COUNT) {
    throw new Error(`${name}：样本数 ${samples.length} ≠ ${COUNT}（fc.sample 去重回退？）`);
  }
  const payload = { seed: SEED, generatedAt, sampleCount: samples.length, samples };
  const path = join(outDir, `${name}.json`);
  writeFileSync(path, JSON.stringify(payload, null, 1));
  const size = JSON.stringify(payload).length;
  total += size;
  console.log(`${name}: ${samples.length} samples, ${(size / 1024).toFixed(1)} KiB`);
}
console.log(`total ${(total / 1024).toFixed(1)} KiB（限额 1 MiB）`);
