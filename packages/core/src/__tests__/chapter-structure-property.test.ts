import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import fc from "fast-check";
import {
  structureInvariant,
  undoEligible,
  type ChapterFingerprint,
  type StructureOperationRecord,
  type StructureOp,
} from "../state/chapter-structure.js";

// R62（1691 号，让号：并行会话先占 1690 做 8 组广覆盖性质+zip-slip 真缺陷修复，
// 本轮做 structureInvariant 深形态专项与其互补）：结构不变量属性语料升级。两层互补：
// ①活体性质（fast-check，每轮新随机种子）：构造性 soundness（合法变换永不误报）
//   + 反篡改（非法变换必点名、消息逐字）+ 形态专项（重复指纹多重集计数下降、
//   undoEligible 同多重集异序）——专咬 golden 手写向量覆盖不到的形态
//   （structure-invariant-vectors.json 8 条指纹全唯一、无同侧多违反、undoEligible 前后同序）。
// ②LCG 共享语料（golden/structure-property-corpus.json）：种子语料两腿同读逐字
//   parity（本文件读 cases；产品引擎 chapter_structure.rs include_str! 同读），
//   指纹字母表 4 值强制重复、size 0..8 全边界；生成算法见该文件 _comment。

const corpusPath = new URL(
  "../../../studio/src/__tests__/golden/structure-property-corpus.json",
  import.meta.url,
);
const corpus = JSON.parse(readFileSync(corpusPath, "utf-8")) as {
  cases: ReadonlyArray<{
    name: string;
    op: StructureOp;
    before: ReadonlyArray<ChapterFingerprint>;
    after: ReadonlyArray<ChapterFingerprint>;
    ok: boolean;
    violations: ReadonlyArray<string>;
  }>;
};

const hexa8 = fc.stringMatching(/^[\da-f]{8}$/);
const jitterArb = fc.array(fc.nat(255), { minLength: 8, maxLength: 8 });
const manifestArb = fc
  .array(hexa8, { minLength: 1, maxLength: 8 })
  .map((fps) =>
    fps.map((fingerprint, i) => ({
      number: i + 1,
      fileName: `ch${String(i + 1).padStart(2, "0")}.md`,
      fingerprint,
    })),
  );

/** jitter 驱动的 Fisher-Yates：逐样本确定、跨样本随机（fc v4 已删 shuffle，不依赖被删 API）。 */
function shuffledWith<T>(items: ReadonlyArray<T>, jitter: ReadonlyArray<number>): T[] {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) {
    const j = jitter[i % jitter.length]! % (i + 1);
    const tmp = out[i]!;
    out[i] = out[j]!;
    out[j] = tmp;
  }
  return out;
}

/** 合法 split 盘面：被拆号换头指纹、新号 at+1 携尾指纹、其余 +1 移位保指纹。 */
function splitAfter(
  before: ReadonlyArray<ChapterFingerprint>,
  at: number,
  headFp: string,
  tailFp: string,
): ChapterFingerprint[] {
  const after = before.map((item) =>
    item.number === at
      ? { ...item, fingerprint: headFp }
      : item.number > at
        ? { ...item, number: item.number + 1 }
        : { ...item },
  );
  after.push({ number: at + 1, fileName: "ch-new.md", fingerprint: tailFp });
  return after;
}

/** 合法 merge 盘面：被并号消失、keep 号（removed-1）覆写、其余 -1 移位保指纹。 */
function mergeAfter(
  before: ReadonlyArray<ChapterFingerprint>,
  removed: number,
  keepFp: string,
): ChapterFingerprint[] {
  return before.flatMap((item) => {
    if (item.number === removed) return [];
    if (item.number === removed - 1) return [{ ...item, fingerprint: keepFp }];
    return [item.number > removed ? { ...item, number: item.number - 1 } : { ...item }];
  });
}

function recordFor(
  kind: StructureOperationRecord["kind"],
  before: ReadonlyArray<ChapterFingerprint>,
  after: ReadonlyArray<ChapterFingerprint>,
): StructureOperationRecord {
  return { ts: "2026-11-26T00:00:00.000Z", opId: "op-1", kind, before, after, undone: false };
}

describe("structureInvariant 活体性质（fast-check）", () => {
  it("reorder: any permutation of a manifest passes (order in the after array is irrelevant)", () => {
    fc.assert(
      fc.property(manifestArb, jitterArb, (manifest, jitter) => {
        const result = structureInvariant({ kind: "reorder" }, manifest, shuffledWith(manifest, jitter));
        expect(result.ok).toBe(true);
        expect(result.violations).toEqual([]);
      }),
    );
  });

  it("reorder: tampering one fingerprint yields exactly the missing/no-match pair naming every owner", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(7), hexa8, (manifest, pick, fresh) => {
        fc.pre(!manifest.some((item) => item.fingerprint === fresh));
        const victim = manifest[pick % manifest.length]!;
        const after = manifest.map((item) => (item === victim ? { ...item, fingerprint: fresh } : { ...item }));
        const result = structureInvariant({ kind: "reorder" }, manifest, after);
        expect(result.ok).toBe(false);
        expect(result.violations).toHaveLength(2);
        const owners = manifest
          .filter((item) => item.fingerprint === victim.fingerprint)
          .map((item) => `${item.number} (${item.fileName})`)
          .join(", ");
        expect(result.violations).toContainEqual(
          `fingerprint ${victim.fingerprint} (chapter ${owners}) is missing from the after set but reorder must not rewrite any chapter`,
        );
        expect(result.violations).toContainEqual(
          `fingerprint ${fresh} (after chapter ${victim.number} (${victim.fileName})) matches no before chapter`,
        );
      }),
    );
  });

  it("split: any head/tail fingerprints pass after renumbering (array order irrelevant)", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(7), hexa8, hexa8, jitterArb, (manifest, pick, headFp, tailFp, jitter) => {
        const at = 1 + (pick % manifest.length);
        const result = structureInvariant(
          { kind: "split", at },
          manifest,
          shuffledWith(splitAfter(manifest, at, headFp, tailFp), jitter),
        );
        expect(result.ok).toBe(true);
        expect(result.violations).toEqual([]);
      }),
    );
  });

  it("split: tampering a shifted chapter names it with its before number and file", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(6), hexa8, hexa8, hexa8, (manifest, pick, headFp, tailFp, fresh) => {
        fc.pre(manifest.length >= 2);
        fc.pre(!manifest.some((item) => item.fingerprint === fresh));
        const at = 1 + (pick % (manifest.length - 1));
        const shifted = manifest.find((item) => item.number > at)!;
        const after = splitAfter(manifest, at, headFp, tailFp).map((item) =>
          item.number === shifted.number + 1 ? { ...item, fingerprint: fresh } : { ...item },
        );
        const result = structureInvariant({ kind: "split", at }, manifest, after);
        expect(result.ok).toBe(false);
        expect(result.violations).toEqual([
          `chapter ${shifted.number} (${shifted.fileName}) content changed (${shifted.fingerprint} -> ${fresh}) but split only renumbers it`,
        ]);
      }),
    );
  });

  it("split: dropping the new tail is caught by the count line alone", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(7), hexa8, (manifest, pick, headFp) => {
        const at = 1 + (pick % manifest.length);
        const after = splitAfter(manifest, at, headFp, "tail").filter((item) => item.number !== at + 1);
        const result = structureInvariant({ kind: "split", at }, manifest, after);
        expect(result.ok).toBe(false);
        expect(result.violations).toEqual([
          `chapter count changed: ${manifest.length} -> ${manifest.length} (split must add exactly one)`,
        ]);
      }),
    );
  });

  it("merge: any keep fingerprint passes after renumbering (array order irrelevant)", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(7), hexa8, jitterArb, (manifest, pick, keepFp, jitter) => {
        fc.pre(manifest.length >= 2);
        const removed = 2 + (pick % (manifest.length - 1));
        const result = structureInvariant(
          { kind: "merge", removed },
          manifest,
          shuffledWith(mergeAfter(manifest, removed, keepFp), jitter),
        );
        expect(result.ok).toBe(true);
        expect(result.violations).toEqual([]);
      }),
    );
  });

  it("merge: tampering a shifted chapter names it; keep chapter rewriting stays exempt", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(6), hexa8, hexa8, (manifest, pick, keepFp, fresh) => {
        fc.pre(manifest.length >= 3);
        fc.pre(!manifest.some((item) => item.fingerprint === fresh));
        const removed = 2 + (pick % (manifest.length - 2)); // 2..size-1 → 存在移位章
        const shifted = manifest.find((item) => item.number > removed)!;
        const after = mergeAfter(manifest, removed, keepFp).map((item) =>
          item.number === shifted.number - 1 ? { ...item, fingerprint: fresh } : { ...item },
        );
        const result = structureInvariant({ kind: "merge", removed }, manifest, after);
        expect(result.ok).toBe(false);
        expect(result.violations).toEqual([
          `chapter ${shifted.number} (${shifted.fileName}) content changed (${shifted.fingerprint} -> ${fresh}) but merge only renumbers it`,
        ]);
      }),
    );
  });

  it("merge: an extra foreign chapter is caught by the count + unexpected lines", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(7), hexa8, (manifest, pick, keepFp) => {
        fc.pre(manifest.length >= 2);
        const removed = 2 + (pick % (manifest.length - 1));
        // 外来章号取 size+1——expectedNumbers ⊆ {1..size-1}，恒不相撞 → 稳定走 unexpected 线。
        const after = [
          ...mergeAfter(manifest, removed, keepFp),
          { number: manifest.length + 1, fileName: "ch-ghost.md", fingerprint: "ghost" },
        ];
        const result = structureInvariant({ kind: "merge", removed }, manifest, after);
        expect(result.ok).toBe(false);
        expect(result.violations).toHaveLength(2);
        expect(result.violations).toContainEqual(
          `chapter count changed: ${manifest.length} -> ${manifest.length} (merge must remove exactly one)`,
        );
        expect(result.violations).toContainEqual(`unexpected chapter ${manifest.length + 1} (ch-ghost.md) survived the merge`);
      }),
    );
  });

  it("merge: a resurrected removed number never passes even when it collides with a shifted slot", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(7), hexa8, (manifest, pick, keepFp) => {
        fc.pre(manifest.length >= 2);
        const removed = 2 + (pick % (manifest.length - 1));
        // 复活号=removed：removed<size 时与移位章新号相撞（生产清单按号收集不会出现该形态，
        // 合成专钉「必拒」——counterpart 查找后写覆盖走 content-changed 线；removed=size 无移位
        // 章不相撞走 unexpected 线。两分支只钉 count 线逐字 + 违反提及 ghost，不冻结 map 内部序）。
        const after = [
          ...mergeAfter(manifest, removed, keepFp),
          { number: removed, fileName: "ch-ghost.md", fingerprint: "ghost" },
        ];
        const result = structureInvariant({ kind: "merge", removed }, manifest, after);
        expect(result.ok).toBe(false);
        expect(result.violations).toHaveLength(2);
        expect(result.violations).toContainEqual(
          `chapter count changed: ${manifest.length} -> ${manifest.length} (merge must remove exactly one)`,
        );
        expect(result.violations.some((v) => v.includes("ghost"))).toBe(true);
      }),
    );
  });

  it("reorder: losing one copy of a duplicated fingerprint names every owner (golden vectors are all-unique)", () => {
    const before: ChapterFingerprint[] = [1, 2, 3].map((n) => ({
      number: n,
      fileName: `ch0${n}.md`,
      fingerprint: n % 2 === 1 ? "aa" : "bb",
    }));
    fc.assert(
      fc.property(hexa8, (fresh) => {
        fc.pre(fresh !== "aa" && fresh !== "bb");
        const after = before.map((item) => (item.number === 1 ? { ...item, fingerprint: fresh } : { ...item }));
        const result = structureInvariant({ kind: "reorder" }, before, after);
        expect(result.ok).toBe(false);
        expect(result.violations).toHaveLength(2);
        expect(result.violations).toContainEqual(
          "fingerprint aa (chapter 1 (ch01.md), 3 (ch03.md)) is missing from the after set but reorder must not rewrite any chapter",
        );
        expect(result.violations).toContainEqual(
          `fingerprint ${fresh} (after chapter 1 (ch01.md)) matches no before chapter`,
        );
      }),
    );
  });
});

describe("undoEligible 活体性质（fast-check）", () => {
  it("same multiset in a different order stays eligible (golden vector only pins the same order)", () => {
    fc.assert(
      fc.property(fc.array(hexa8, { minLength: 2, maxLength: 6 }), jitterArb, (fps, jitter) => {
        const after = fps.map((fingerprint, i) => ({
          number: i + 1,
          fileName: `ch${String(i + 1).padStart(2, "0")}.md`,
          fingerprint,
        }));
        const tail = recordFor("reorder", after, after);
        const verdict = undoEligible(tail, shuffledWith(after, jitter));
        expect(verdict.eligible).toBe(true);
        expect(verdict.reason).toBeNull();
      }),
    );
  });

  it("every sound split/merge/reorder trio leaves an undoable tail", () => {
    fc.assert(
      fc.property(manifestArb, fc.nat(7), hexa8, hexa8, (manifest, pick, headFp, tailFp) => {
        expect(undoEligible(recordFor("reorder", manifest, manifest), manifest)).toEqual({
          eligible: true,
          reason: null,
        });
        const at = 1 + (pick % manifest.length);
        const splitSet = splitAfter(manifest, at, headFp, tailFp);
        expect(undoEligible(recordFor("split", manifest, splitSet), splitSet)).toEqual({
          eligible: true,
          reason: null,
        });
        if (manifest.length >= 2) {
          const removed = 2 + (pick % (manifest.length - 1));
          const mergeSet = mergeAfter(manifest, removed, headFp);
          expect(undoEligible(recordFor("merge", manifest, mergeSet), mergeSet)).toEqual({
            eligible: true,
            reason: null,
          });
        }
      }),
    );
  });

  it("tampering one fingerprint after the operation refuses with the drift reason", () => {
    fc.assert(
      fc.property(manifestArb, hexa8, (manifest, fresh) => {
        fc.pre(!manifest.some((item) => item.fingerprint === fresh));
        const drifted = manifest.map((item, i) => (i === 0 ? { ...item, fingerprint: fresh } : { ...item }));
        const verdict = undoEligible(recordFor("reorder", manifest, manifest), drifted);
        expect(verdict).toEqual({ eligible: false, reason: "disk state changed since the last operation" });
      }),
    );
  });
});

describe("structureInvariant LCG 共享语料（两腿同读逐字 parity）", () => {
  it(`all ${corpus.cases.length} seeded cases match byte-for-byte`, () => {
    for (const c of corpus.cases) {
      const result = structureInvariant(c.op, c.before, c.after);
      expect(result.violations, `case: ${c.name}`).toEqual(c.violations);
      expect(result.ok, `case: ${c.name}`).toBe(c.ok);
    }
  });
});
