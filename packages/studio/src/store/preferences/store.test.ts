import { describe, it, expect, beforeEach } from "vitest";
import {
  readStoredToolDetailsDefaultOpen,
  readStoredNavLayoutV2,
  readStoredActiveNavSection,
  usePreferencesStore,
  TOOL_DETAILS_STORAGE_KEY,
  NAV_LAYOUT_V2_STORAGE_KEY,
  ACTIVE_NAV_SECTION_STORAGE_KEY,
} from "./store";

function fakeStorage(entries: Record<string, string>) {
  return {
    getItem: (key: string) => (key in entries ? entries[key] : null),
  };
}

describe("readStoredToolDetailsDefaultOpen", () => {
  it("defaults to true when no storage is available", () => {
    expect(readStoredToolDetailsDefaultOpen(null)).toBe(true);
    expect(readStoredToolDetailsDefaultOpen(undefined)).toBe(true);
  });

  it("defaults to true when nothing is stored", () => {
    expect(readStoredToolDetailsDefaultOpen(fakeStorage({}))).toBe(true);
  });

  it("returns false only for an explicitly stored \"false\"", () => {
    expect(readStoredToolDetailsDefaultOpen(fakeStorage({ [TOOL_DETAILS_STORAGE_KEY]: "false" }))).toBe(false);
    expect(readStoredToolDetailsDefaultOpen(fakeStorage({ [TOOL_DETAILS_STORAGE_KEY]: "true" }))).toBe(true);
    expect(readStoredToolDetailsDefaultOpen(fakeStorage({ [TOOL_DETAILS_STORAGE_KEY]: "garbage" }))).toBe(true);
  });
});

describe("readStoredNavLayoutV2", () => {
  it("defaults to false — the V1 sidebar stays the default track", () => {
    expect(readStoredNavLayoutV2(null)).toBe(false);
    expect(readStoredNavLayoutV2(fakeStorage({}))).toBe(false);
  });

  it("opts in only via an explicitly stored \"true\"", () => {
    expect(readStoredNavLayoutV2(fakeStorage({ [NAV_LAYOUT_V2_STORAGE_KEY]: "true" }))).toBe(true);
    expect(readStoredNavLayoutV2(fakeStorage({ [NAV_LAYOUT_V2_STORAGE_KEY]: "1" }))).toBe(false);
  });
});

describe("readStoredActiveNavSection", () => {
  it("returns null for missing or invalid values (follow the route)", () => {
    expect(readStoredActiveNavSection(null)).toBeNull();
    expect(readStoredActiveNavSection(fakeStorage({}))).toBeNull();
    expect(readStoredActiveNavSection(fakeStorage({ [ACTIVE_NAV_SECTION_STORAGE_KEY]: "nope" }))).toBeNull();
  });

  it("restores a valid stored section", () => {
    expect(readStoredActiveNavSection(fakeStorage({ [ACTIVE_NAV_SECTION_STORAGE_KEY]: "tools" }))).toBe("tools");
    expect(readStoredActiveNavSection(fakeStorage({ [ACTIVE_NAV_SECTION_STORAGE_KEY]: "film" }))).toBe("film");
  });
});

describe("usePreferencesStore", () => {
  beforeEach(() => {
    usePreferencesStore.setState({ toolDetailsDefaultOpen: true, navLayoutV2: false, activeNavSection: null });
  });

  it("starts with details expanded by default", () => {
    expect(usePreferencesStore.getState().toolDetailsDefaultOpen).toBe(true);
  });

  it("setToolDetailsDefaultOpen updates the state", () => {
    usePreferencesStore.getState().setToolDetailsDefaultOpen(false);
    expect(usePreferencesStore.getState().toolDetailsDefaultOpen).toBe(false);

    usePreferencesStore.getState().setToolDetailsDefaultOpen(true);
    expect(usePreferencesStore.getState().toolDetailsDefaultOpen).toBe(true);
  });

  it("setNavLayoutV2 and setActiveNavSection update the state", () => {
    usePreferencesStore.getState().setNavLayoutV2(true);
    expect(usePreferencesStore.getState().navLayoutV2).toBe(true);

    usePreferencesStore.getState().setActiveNavSection("manage");
    expect(usePreferencesStore.getState().activeNavSection).toBe("manage");

    usePreferencesStore.getState().setActiveNavSection(null);
    expect(usePreferencesStore.getState().activeNavSection).toBeNull();
  });
});
