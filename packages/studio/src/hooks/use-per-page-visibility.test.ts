import { describe, expect, it } from "vitest";
import { readStoredVisibilityMap } from "./use-per-page-visibility";

describe("readStoredVisibilityMap", () => {
  it("parses a valid per-page map", () => {
    const storage = { getItem: () => JSON.stringify({ book: true, chat: false }) };
    expect(readStoredVisibilityMap(storage, "k")).toEqual({ book: true, chat: false });
  });

  it("degrades to empty for missing/invalid input and non-boolean values", () => {
    expect(readStoredVisibilityMap(null, "k")).toEqual({});
    expect(readStoredVisibilityMap({ getItem: () => null }, "k")).toEqual({});
    expect(readStoredVisibilityMap({ getItem: () => "{oops" }, "k")).toEqual({});
    expect(readStoredVisibilityMap({ getItem: () => "[1,2]" }, "k")).toEqual({});
    expect(readStoredVisibilityMap({ getItem: () => JSON.stringify({ book: "yes", chat: true }) }, "k"))
      .toEqual({ chat: true });
  });
});
