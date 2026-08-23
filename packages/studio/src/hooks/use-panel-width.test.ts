import { describe, expect, it } from "vitest";
import {
  DEFAULT_PANEL_WIDTH,
  MAX_PANEL_WIDTH,
  MIN_PANEL_WIDTH,
  clampPanelWidth,
  readStoredPanelWidth,
} from "./use-panel-width";

describe("clampPanelWidth", () => {
  it("clamps to the 180–400 range", () => {
    expect(clampPanelWidth(100)).toBe(MIN_PANEL_WIDTH);
    expect(clampPanelWidth(999)).toBe(MAX_PANEL_WIDTH);
    expect(clampPanelWidth(260)).toBe(260);
    expect(clampPanelWidth(320.6)).toBe(321);
  });

  it("falls back to the default for invalid input", () => {
    expect(clampPanelWidth(Number.NaN)).toBe(DEFAULT_PANEL_WIDTH);
    expect(clampPanelWidth(Number.POSITIVE_INFINITY)).toBe(DEFAULT_PANEL_WIDTH);
  });
});

describe("readStoredPanelWidth", () => {
  it("returns the default when nothing valid is stored", () => {
    expect(readStoredPanelWidth(null)).toBe(DEFAULT_PANEL_WIDTH);
    expect(readStoredPanelWidth({ getItem: () => null })).toBe(DEFAULT_PANEL_WIDTH);
    expect(readStoredPanelWidth({ getItem: () => "abc" })).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("restores a stored width and clamps stale values into range", () => {
    expect(readStoredPanelWidth({ getItem: () => "320" })).toBe(320);
    expect(readStoredPanelWidth({ getItem: () => "5000" })).toBe(MAX_PANEL_WIDTH);
  });
});
