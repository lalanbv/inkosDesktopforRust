import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  artifactEditDraftKey,
  clearArtifactEditDraft,
  loadArtifactEditDraft,
  saveArtifactEditDraft,
} from "./artifact-edit-draft";

// 998 号：编辑草稿 sessionStorage 持久化的 helper 单测（node 环境——内存 stub
// 走「sessionStorage 存在」的真实浏览器路径；实现侧 typeof 卫兵由 stub 缺席态
// 隐式覆盖：beforeEach 前/afterEach 后 sessionStorage 未定义 → load 返回 null）。
describe("artifact-edit-draft helpers (998)", () => {
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

  it("key 构造：章节与 truth 文件两形互不串扰", () => {
    expect(artifactEditDraftKey("b1", true, 3, null)).toBe("inkos.artifactEdit:b1:ch:3");
    expect(artifactEditDraftKey("b1", false, null, "truth/characters.md")).toBe(
      "inkos.artifactEdit:b1:truth/characters.md",
    );
    expect(artifactEditDraftKey("b2", false, null, "truth/characters.md")).toBe(
      "inkos.artifactEdit:b2:truth/characters.md",
    );
  });

  it("save → load roundtrip；clear 后为 null", () => {
    const key = artifactEditDraftKey("b1", false, null, "truth/characters.md");
    expect(loadArtifactEditDraft(key)).toBeNull();
    saveArtifactEditDraft(key, { editing: true, content: "未保存的输入" });
    expect(loadArtifactEditDraft(key)).toEqual({ editing: true, content: "未保存的输入" });
    clearArtifactEditDraft(key);
    expect(loadArtifactEditDraft(key)).toBeNull();
  });

  it("JSON 损坏 / 字段形状错 → null（回落非编辑态，不抛错）", () => {
    const key = artifactEditDraftKey("b1", false, null, "truth/f.md");
    sessionStorage.setItem(key, "{not-json");
    expect(loadArtifactEditDraft(key)).toBeNull();
    sessionStorage.setItem(key, JSON.stringify({ editing: "yes", content: 42 }));
    expect(loadArtifactEditDraft(key)).toBeNull();
  });
});
