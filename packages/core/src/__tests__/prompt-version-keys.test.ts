import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  appendPromptVersion,
  listPromptVersions,
  loadPromptVersions,
  readPromptVersion,
} from "../utils/prompt-pack-store.js";

/**
 * 1666 号：版本以 (promptId, ts) 定位——同一 prompt 不得出现同 ts 两行。
 * 同一毫秒内的两次写入（并发保存 / 回滚后紧接保存）此前让后一行被前一行
 * 遮蔽：读取只认一条、列表里同 ts 的两行「旧的排在前」。Rust 镜像 =
 * engine-rs/src/prompts/prompt_pack.rs tests。
 */
const roots: string[] = [];
afterEach(async () => {
  for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true });
});
async function freshRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "inkos-prompt-version-keys-"));
  roots.push(root);
  return root;
}

const TS = "2026-11-24T12:00:00.000Z";
const at = (offset: number) => new Date(Date.parse(TS) + offset).toISOString();

describe("prompt 版本定位键 (promptId, ts)", () => {
  it("同一毫秒写入两行：新行 ts 顺推 1ms，两行各可取到、列表最新在前；别的 prompt 不受影响", async () => {
    const root = await freshRoot();
    await appendPromptVersion(root, "p.one", "put", "第一版", TS);
    await appendPromptVersion(root, "p.one", "put", "第二版", TS);
    await appendPromptVersion(root, "p.two", "put", "别的", TS);

    const versions = await listPromptVersions(root, "p.one");
    expect(versions.map((record) => record.ts)).toEqual([at(1), TS]);
    for (const [key, content] of [[TS, "第一版"], [at(1), "第二版"]] as const) {
      expect((await readPromptVersion(root, "p.one", key))?.content, key).toBe(content);
    }
    expect((await readPromptVersion(root, "p.two", TS))?.content).toBe("别的");
  });

  it("并发追加同一 prompt 同一 ts：每行各得唯一键，全部可按 ts 取到", async () => {
    const root = await freshRoot();
    const total = 20;
    await Promise.all(Array.from({ length: total }, (_, index) =>
      appendPromptVersion(root, "p.one", "put", `第 ${index} 版`, TS)));
    const rows = await loadPromptVersions(root);
    expect(rows).toHaveLength(total);
    expect(new Set(rows.map((record) => record.ts)).size).toBe(total);
    for (let index = 0; index < total; index++) {
      // 键按追加先后顺推——但并发下谁先拿到哪个键不确定，只断言「键集合完整 +
      // 每个键都能取到一行」。
      const record = await readPromptVersion(root, "p.one", at(index));
      expect(record, at(index)).toMatchObject({ promptId: "p.one", action: "put" });
    }
    expect(await readPromptVersion(root, "p.one", at(total))).toBeUndefined();
    const listed = await listPromptVersions(root, "p.one");
    expect(listed.map((record) => record.ts)).toEqual(
      Array.from({ length: total }, (_, index) => at(total - 1 - index)),
    );
  });

  it("非 ISO 毫秒形态的 ts 无法顺推：原样写入，不改写调用方给的键", async () => {
    const root = await freshRoot();
    await appendPromptVersion(root, "p.three", "put", "任意键", "v1");
    await appendPromptVersion(root, "p.three", "put", "任意键二", "v1");
    expect((await loadPromptVersions(root)).map((record) => record.ts)).toEqual(["v1", "v1"]);
  });

  it("修复前落盘的同 ts 两行（撞键历史数据）：列表按写入序倒排、读取取最后写入的那行", async () => {
    const root = await freshRoot();
    await mkdir(join(root, "prompt"), { recursive: true });
    const row = (content: string) => JSON.stringify({ ts: TS, promptId: "p.one", action: "put", content });
    await writeFile(join(root, "prompt", ".versions.jsonl"), `${[row("旧的"), row("新的")].join("\n")}\n`, "utf-8");

    expect((await listPromptVersions(root, "p.one")).map((record) => record.content)).toEqual(["新的", "旧的"]);
    expect((await readPromptVersion(root, "p.one", TS))?.content).toBe("新的");
  });
});
