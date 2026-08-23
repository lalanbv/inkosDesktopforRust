import { describe, expect, it } from "vitest";
import {
  cycleThemeMode,
  getTimeBasedThemeForHour,
  readStoredThemeMode,
  resolveEffectiveTheme,
} from "./use-theme";

describe("getTimeBasedThemeForHour", () => {
  it("switches at 6:00 and 18:00", () => {
    expect(getTimeBasedThemeForHour(5)).toBe("dark");
    expect(getTimeBasedThemeForHour(6)).toBe("light");
    expect(getTimeBasedThemeForHour(17)).toBe("light");
    expect(getTimeBasedThemeForHour(18)).toBe("dark");
  });
});

describe("readStoredThemeMode", () => {
  it("accepts the three explicit modes from storage", () => {
    expect(readStoredThemeMode({ getItem: () => "light" })).toBe("light");
    expect(readStoredThemeMode({ getItem: () => "dark" })).toBe("dark");
    expect(readStoredThemeMode({ getItem: () => "auto" })).toBe("auto");
  });

  it("treats junk or missing values as never-chosen", () => {
    expect(readStoredThemeMode({ getItem: () => "blue" })).toBeNull();
    expect(readStoredThemeMode({ getItem: () => null })).toBeNull();
    expect(readStoredThemeMode(null)).toBeNull();
  });
});

describe("resolveEffectiveTheme", () => {
  it("explicit modes win regardless of system or clock", () => {
    expect(resolveEffectiveTheme({ mode: "light", systemPrefersDark: true, hour: 23 })).toBe("light");
    expect(resolveEffectiveTheme({ mode: "dark", systemPrefersDark: false, hour: 9 })).toBe("dark");
  });

  it("auto follows the system preference", () => {
    expect(resolveEffectiveTheme({ mode: "auto", systemPrefersDark: true, hour: 12 })).toBe("dark");
    expect(resolveEffectiveTheme({ mode: "auto", systemPrefersDark: false, hour: 23 })).toBe("light");
  });

  it("keeps the legacy time-based default when nothing was chosen", () => {
    expect(resolveEffectiveTheme({ mode: null, systemPrefersDark: false, hour: 23 })).toBe("dark");
    expect(resolveEffectiveTheme({ mode: null, systemPrefersDark: true, hour: 9 })).toBe("light");
  });
});

describe("cycleThemeMode", () => {
  it("cycles light → dark → auto → light", () => {
    expect(cycleThemeMode("light", "light")).toBe("dark");
    expect(cycleThemeMode("dark", "dark")).toBe("auto");
    expect(cycleThemeMode("auto", "dark")).toBe("light");
  });

  it("first click from never-chosen flips the current effective theme", () => {
    expect(cycleThemeMode(null, "light")).toBe("dark");
    expect(cycleThemeMode(null, "dark")).toBe("light");
  });
});
