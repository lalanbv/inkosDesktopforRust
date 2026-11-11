import { describe, expect, it } from "vitest";
import { insertWithOldestEviction } from "./bounded-map.js";

describe("insertWithOldestEviction（985 号：进程内表有界化惯用形）", () => {
  it("cap 内插入不逐出", () => {
    const map = new Map<string, number>();
    insertWithOldestEviction(map, "a", 1, 5);
    insertWithOldestEviction(map, "b", 2, 5);
    insertWithOldestEviction(map, "c", 3, 5);
    expect(map.size).toBe(3);
    expect(map.get("a")).toBe(1);
    expect(map.get("c")).toBe(3);
  });

  it("超 cap 按插入序逐出最旧键", () => {
    const map = new Map<string, number>();
    insertWithOldestEviction(map, "a", 1, 2);
    insertWithOldestEviction(map, "b", 2, 2);
    insertWithOldestEviction(map, "c", 3, 2);
    expect(map.size).toBe(2);
    expect(map.has("a")).toBe(false);
    expect(map.get("b")).toBe(2);
    expect(map.get("c")).toBe(3);
  });

  it("覆写已有键不增长且重置插入序（被覆写键变最新）", () => {
    const map = new Map<string, number>();
    insertWithOldestEviction(map, "a", 1, 2);
    insertWithOldestEviction(map, "b", 2, 2);
    insertWithOldestEviction(map, "a", 10, 2); // 覆写：size 仍 2，a 变最新
    expect(map.size).toBe(2);
    expect(map.get("a")).toBe(10);
    insertWithOldestEviction(map, "c", 3, 2); // 最旧=b 被逐（a 因覆写保鲜）
    expect(map.size).toBe(2);
    expect(map.has("b")).toBe(false);
    expect(map.get("a")).toBe(10);
    expect(map.get("c")).toBe(3);
  });

  it("cap=1 只保留最新", () => {
    const map = new Map<string, number>();
    insertWithOldestEviction(map, "a", 1, 1);
    insertWithOldestEviction(map, "b", 2, 1);
    expect(map.size).toBe(1);
    expect(map.get("b")).toBe(2);
    expect(map.has("a")).toBe(false);
  });
});
