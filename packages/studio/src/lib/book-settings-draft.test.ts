import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  bookSettingsDraftKey,
  clearBookSettingsDraft,
  loadBookSettingsDraft,
  saveBookSettingsDraft,
} from "./book-settings-draft";

// 1001 号：BookDetail 设置表单草稿 helper 单测（node 环境——内存 sessionStorage
// stub 走真实浏览器路径；实现侧 typeof 卫兵由 stub 缺席态隐式覆盖）。
describe("book-settings-draft helpers (1001)", () => {
  const store = new Map<string, string>();

  beforeEach(() => {
    store.clear();
    (globalThis as Record<string, unknown>).sessionStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    };
  });

  afterEach(() => {
    delete (globalThis as Record<string, unknown>).sessionStorage;
  });

  it("key 按 bookId 隔离：多书互不串扰", () => {
    expect(bookSettingsDraftKey("b1")).toBe("inkos.bookSettingsDraft:b1");
    expect(bookSettingsDraftKey("b2")).toBe("inkos.bookSettingsDraft:b2");
    saveBookSettingsDraft("b1", { wordCount: 2000 });
    expect(loadBookSettingsDraft("b1")).toEqual({ wordCount: 2000 });
    expect(loadBookSettingsDraft("b2")).toBeNull();
  });

  it("save → load roundtrip（部分字段草稿）；clear 后为 null", () => {
    saveBookSettingsDraft("b1", { targetChapters: 12, status: "ongoing" });
    expect(loadBookSettingsDraft("b1")).toEqual({ targetChapters: 12, status: "ongoing" });
    clearBookSettingsDraft("b1");
    expect(loadBookSettingsDraft("b1")).toBeNull();
  });

  it("字段类型错 / 非有限数 / 损坏 JSON → 逐字段过滤或 null", () => {
    const key = bookSettingsDraftKey("b1");
    sessionStorage.setItem(key, JSON.stringify({ wordCount: "2000", targetChapters: 12 }));
    // wordCount 非数字被过滤；targetChapters 合法保留
    expect(loadBookSettingsDraft("b1")).toEqual({ targetChapters: 12 });
    sessionStorage.setItem(key, JSON.stringify({ wordCount: Number.NaN }));
    // NaN 非有限 → 全过滤 → 空对象 → null
    expect(loadBookSettingsDraft("b1")).toBeNull();
    sessionStorage.setItem(key, "{broken");
    expect(loadBookSettingsDraft("b1")).toBeNull();
  });
});
