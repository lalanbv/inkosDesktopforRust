import fc from "fast-check";
import { describe, expect, it } from "vitest";
import { docxEscapeXml, mdZipEntryPath } from "../interaction/export-artifact.js";
import { dailyWordDelta, expectedFinish, goalStreak, shiftDayKey } from "../utils/writing-goal.js";
import { usageProjection } from "../utils/usage-budget.js";
import { structureInvariant, type ChapterFingerprint, type StructureOp } from "../state/chapter-structure.js";

// R62/1690 号：八条性质的 fast-check 原生断言（运行时随机，性质恒真应稳定；
// 偶发反例=真缺陷）。与落盘语料（property-corpus-replay）互补：语料锁基线、
// 原生断言探索新反例。生成器同 scripts/generate-property-corpus.mjs 的形态。

const unescapeXml = (text: string): string =>
  text
    .replaceAll("&apos;", "'")
    .replaceAll("&quot;", '"')
    .replaceAll("&gt;", ">")
    .replaceAll("&lt;", "<")
    .replaceAll("&amp;", "&");

const pad = (n: number, width: number) => String(n).padStart(width, "0");
const dayKeyArb = fc.record({
  year: fc.integer({ min: 2000, max: 2030 }),
  month: fc.integer({ min: 1, max: 12 }),
  day: fc.integer({ min: 1, max: 28 }),
}).map(({ year, month, day }) => `${year}-${pad(month, 2)}-${pad(day, 2)}`);

const chapterList = fc
  .array(fc.integer({ min: 0, max: 0xffffff }), { minLength: 1, maxLength: 8 })
  .map((seeds) =>
    seeds.map((seed, index) => ({
      number: index + 1,
      fileName: `${pad(index + 1, 4)}_章${index + 1}.md`,
      fingerprint: `${seed.toString(16)}-${index + 1}`,
    })),
  );
const opArb = fc.constantFrom("reorder", "split", "merge");
const mutateArb = fc.constantFrom("none", "none", "drop", "fingerprint");

function buildAfter(op: string, before: ChapterFingerprint[], mutate: string): ChapterFingerprint[] {
  const at = Math.min(before.length, Math.max(1, Math.floor(before.length / 2) + 1));
  let after: ChapterFingerprint[];
  if (op === "reorder") {
    after = [...before].reverse();
  } else if (op === "split") {
    after = [];
    for (const item of before) {
      if (item.number < at) after.push(item);
      else if (item.number === at) {
        after.push({ ...item, fingerprint: `${item.fingerprint}-head` });
        after.push({ number: item.number + 1, fileName: `${pad(item.number + 1, 4)}_新.md`, fingerprint: `${item.fingerprint}-novel` });
      } else {
        after.push({ ...item, number: item.number + 1, fileName: item.fileName.replace(pad(item.number, 4), pad(item.number + 1, 4)) });
      }
    }
  } else {
    const removed = Math.min(before.length, 2);
    after = [];
    for (const item of before) {
      if (item.number === removed) continue;
      const target = item.number > removed ? item.number - 1 : item.number;
      const merged = target === removed - 1;
      after.push({
        ...item,
        number: target,
        fileName: merged ? item.fileName.replace(pad(item.number, 4), pad(target, 4)) : item.fileName,
        fingerprint: merged ? `${item.fingerprint}-merged` : item.fingerprint,
      });
    }
  }
  if (mutate === "drop" && after.length > 0) after = after.slice(1);
  if (mutate === "fingerprint") {
    // 注入选「指纹在 before 多重集中」的项（移位章）——keep/被删章的指纹变化
    // 是允许语义（合并覆写/章被删），注入它们不可观测。
    const beforeFps = new Set(before.map((item) => item.fingerprint));
    for (let index = after.length - 1; index >= 0; index -= 1) {
      if (beforeFps.has(after[index].fingerprint)) {
        after[index] = { ...after[index], fingerprint: `${after[index].fingerprint}-mut` };
        break;
      }
    }
  }
  return after;
}

describe("R62 八条性质（fast-check 原生断言）", () => {
  it("①docxEscapeXml：unescape(escape(x)) == x，输出无裸 & < >", () => {
    fc.assert(
      fc.property(fc.string({ maxLength: 200 }), (text) => {
        const escaped = docxEscapeXml(text);
        expect(escaped).not.toMatch(/&(?!amp;|lt;|gt;|quot;|apos;)/u);
        expect(escaped).not.toContain("<");
        expect(escaped).not.toContain(">");
        expect(unescapeXml(escaped)).toBe(text);
      }),
    );
  });

  it("②mdZipEntryPath：不含 .. 段、不以 / 开头、不含反斜杠与非法字符", () => {
    fc.assert(
      fc.property(
        fc.option(fc.string({ minLength: 1, maxLength: 16 }), { nil: undefined }),
        fc.array(fc.record({ id: fc.string({ minLength: 1, maxLength: 16 }), title: fc.string({ maxLength: 24 }), order: fc.integer({ min: 1, max: 99 }) }), { maxLength: 4 }),
        fc.integer({ min: 1, max: 9999 }),
        fc.string({ maxLength: 32 }),
        (volumeId, volumes, chapterNumber, chapterTitle) => {
          const path = mdZipEntryPath(volumeId, volumes, chapterNumber, chapterTitle);
          expect(path).not.toMatch(/\\/u);
          expect(path).not.toMatch(/^\//u);
          expect(path).not.toContain("..");
          // 非法字符（zip 保留集）不得出现在任何段内（/ 是分隔符除外）。
          expect(path.split("/").every((segment) => !/[\\:*?"<>|]/u.test(segment))).toBe(true);
        },
      ),
    );
  });

  it("③dailyWordDelta：行流置换不变，全表求和等于 Σ(after−before)", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.record({
            ts: dayKeyArb.map((day) => `${day}T12:00:00.000Z`),
            chapterNum: fc.integer({ min: 1, max: 99 }),
            before: fc.integer({ min: 0, max: 50000 }),
            after: fc.integer({ min: 0, max: 50000 }),
          }),
          { maxLength: 30 },
        ),
        fc.integer({ min: 0, max: 1000000 }),
        (rows, shuffleSeed) => {
          const dayKeyOf = (row: { ts: string }) => row.ts.slice(0, 10);
          const baseline = dailyWordDelta(rows, dayKeyOf);
          const shuffled = [...rows].sort(() => (shuffleSeed % 2 === 0 ? -1 : 1) * (Number(shuffleSeed) % 3));
          const permuted = dailyWordDelta(shuffled, dayKeyOf);
          expect(permuted).toEqual(baseline);
          const totalOf = (list: typeof baseline) => list.reduce((sum, entry) => sum + entry.delta, 0);
          const rawTotal = rows.reduce((sum, row) => sum + (row.after - row.before), 0);
          expect(totalOf(baseline)).toBe(rawTotal);
          expect(totalOf(permuted)).toBe(rawTotal);
        },
      ),
    );
  });

  it("④goalStreak：结果不超过表长，计入的每一天净增不小于目标", () => {
    fc.assert(
      fc.property(
        fc.array(fc.record({ dayKey: dayKeyArb, delta: fc.integer({ min: -500, max: 3000 }) }), { maxLength: 12 }),
        fc.integer({ min: 1, max: 2000 }),
        dayKeyArb,
        (deltas, dailyGoalWords, todayKey) => {
          const deduped = [...new Map(deltas.map((entry) => [entry.dayKey, entry.delta])).entries()]
            .map(([dayKey, delta]) => ({ dayKey, delta }))
            .sort((a, b) => (a.dayKey < b.dayKey ? -1 : 1));
          const streak = goalStreak(deduped, dailyGoalWords, todayKey);
          expect(streak).toBeLessThanOrEqual(deduped.length);
          const byDay = new Map(deduped.map((entry) => [entry.dayKey, entry.delta]));
          let endpoint = deduped.at(-1)?.dayKey ?? todayKey;
          if (endpoint === todayKey && (byDay.get(todayKey) ?? 0) < dailyGoalWords) endpoint = shiftDayKey(todayKey, -1);
          let day = endpoint;
          for (let index = 0; index < streak; index += 1) {
            expect((byDay.get(day) ?? 0)).toBeGreaterThanOrEqual(dailyGoalWords);
            day = shiftDayKey(day, -1);
          }
        },
      ),
    );
  });

  it("⑤usageProjection：各服务之和等于全部行之和（daily/monthly/total 三口径）", () => {
    fc.assert(
      fc.property(
        fc.array(
          fc.record({
            serviceKey: fc.constantFrom("openai", "anthropic", "custom-x", "kkaiapi"),
            model: fc.constantFrom("m1", "m2"),
            promptTokens: fc.integer({ min: 0, max: 50000 }),
            completionTokens: fc.integer({ min: 0, max: 50000 }),
            totalTokens: fc.integer({ min: 0, max: 100000 }),
            ts: dayKeyArb.map((day) => `${day}T12:00:00.000Z`),
          }),
          { maxLength: 30 },
        ),
        dayKeyArb,
        (rows, todayKey) => {
          const dayKeyOf = (row: { ts: string }) => row.ts.slice(0, 10);
          const todayMonth = todayKey.slice(0, 7);
          const projection = usageProjection(rows, dayKeyOf, todayKey);
          const sumOf = (pick: (usage: { daily: number; monthly: number; total: number }) => number) =>
            [...projection.values()].reduce((sum, usage) => sum + pick(usage), 0);
          expect(sumOf((usage) => usage.total)).toBe(rows.reduce((sum, row) => sum + row.totalTokens, 0));
          expect(sumOf((usage) => usage.daily)).toBe(rows.filter((row) => dayKeyOf(row) === todayKey).reduce((sum, row) => sum + row.totalTokens, 0));
          expect(sumOf((usage) => usage.monthly)).toBe(rows.filter((row) => dayKeyOf(row).slice(0, 7) === todayMonth).reduce((sum, row) => sum + row.totalTokens, 0));
        },
      ),
    );
  });

  it("⑥expectedFinish：已达成或节奏≤0 为空；其余完成日=今天+ceil(剩余/节奏)", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 100000 }),
        fc.integer({ min: 0, max: 100000 }),
        fc.integer({ min: -10, max: 5000 }),
        dayKeyArb,
        (currentWords, targetWords, dailyPaceWords, todayKey) => {
          const finish = expectedFinish(currentWords, targetWords, dailyPaceWords, todayKey);
          if (currentWords >= targetWords || dailyPaceWords <= 0) {
            expect(finish).toBeUndefined();
            return;
          }
          const remaining = Math.ceil((targetWords - currentWords) / dailyPaceWords);
          expect(finish).toBe(shiftDayKey(todayKey, remaining));
        },
      ),
    );
  });

  it("⑦shiftDayKey：shift(+n) 再 shift(−n) 等于原值", () => {
    fc.assert(
      fc.property(dayKeyArb, fc.integer({ min: -800, max: 800 }), (dayKey, days) => {
        expect(shiftDayKey(shiftDayKey(dayKey, days), -days)).toBe(dayKey);
      }),
    );
  });

  it("⑧structureInvariant：随机重排通过、随机丢成员违反", () => {
    fc.assert(
      fc.property(chapterList, opArb, mutateArb, (before, op, mutate) => {
        const structureOp: StructureOp = op === "split"
          ? { kind: "split", at: Math.min(before.length, Math.max(1, Math.floor(before.length / 2) + 1)) }
          : op === "merge"
            ? { kind: "merge", removed: Math.min(before.length, 2) }
            : { kind: "reorder" };
        const after = buildAfter(op, before, mutate);
        const { violations } = structureInvariant(structureOp, before, after);
        // 注入可观测前提：存在移位章（其指纹在 before 多重集中）——reorder 恒有；
        // split 无移位章当 before.length ≤ at；merge 无移位章当 before.length ≤ removed。
        const shiftedExists = structureOp.kind === "reorder"
          ? before.length > 0
          : structureOp.kind === "split"
            ? before.length > structureOp.at
            : before.length > structureOp.removed;
        if (mutate === "none") {
          expect(violations, `${op} 恒通过`).toEqual([]);
        } else if (!shiftedExists) {
          // 退化组合：无移位章可注入（removed/at 即尾部唯一章）——不可观测，跳过。
          return;
        } else {
          expect(violations.length, `${op} 注入 ${mutate} 必违反`).toBeGreaterThan(0);
        }
      }),
    );
  });
});
