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
    // 1008 号：status 样例改用 BookStatus 枚举真源值——原样例 "ongoing" 不在
    // core BookStatusSchema 枚举内（枚举盲区自证：测试自身认证过非法值可往返）。
    saveBookSettingsDraft("b1", { targetChapters: 12, status: "active" });
    expect(loadBookSettingsDraft("b1")).toEqual({ targetChapters: 12, status: "active" });
    clearBookSettingsDraft("b1");
    expect(loadBookSettingsDraft("b1")).toBeNull();
  });

  it("1008：status 非法值（不在 BookStatus 枚举）被过滤——sessionStorage 篡改/旧协议残留不进表单", () => {
    const key = bookSettingsDraftKey("b1");
    sessionStorage.setItem(key, JSON.stringify({ status: "ongoing" }));
    // 过滤后无剩余字段 → 整体 null（回落真源值）
    expect(loadBookSettingsDraft("b1")).toBeNull();
    // 合法值与其它字段共存时只过滤非法 status
    sessionStorage.setItem(key, JSON.stringify({ wordCount: 2000, status: "bogus-status" }));
    expect(loadBookSettingsDraft("b1")).toEqual({ wordCount: 2000 });
  });

  it("1008：core 枚举全六值均合法（incubating 曾被 BookDetail 本地漂移 union 丢失）", () => {
    const key = bookSettingsDraftKey("b1");
    for (const status of ["incubating", "outlining", "active", "paused", "completed", "dropped"]) {
      sessionStorage.setItem(key, JSON.stringify({ status }));
      expect(loadBookSettingsDraft("b1")).toEqual({ status });
    }
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
