import { describe, expect, it } from "vitest";
import { formatCombo, parseCombo, shouldIgnore } from "./use-global-hotkeys";

function keyEvent(overrides: Partial<Parameters<typeof parseCombo>[0]> & { key: string }) {
  return {
    key: overrides.key,
    metaKey: overrides.metaKey ?? false,
    ctrlKey: overrides.ctrlKey ?? false,
    altKey: overrides.altKey ?? false,
    shiftKey: overrides.shiftKey ?? false,
  };
}

describe("parseCombo", () => {
  it("normalizes Meta and Ctrl to mod for cross-platform combos", () => {
    expect(parseCombo(keyEvent({ key: "k", metaKey: true }))).toBe("mod+k");
    expect(parseCombo(keyEvent({ key: "k", ctrlKey: true }))).toBe("mod+k");
  });

  it("keeps a stable part order: mod + alt + shift + key", () => {
    expect(parseCombo(keyEvent({ key: "p", metaKey: true, shiftKey: true }))).toBe("mod+shift+p");
    expect(parseCombo(keyEvent({ key: "p", ctrlKey: true, altKey: true, shiftKey: true }))).toBe("mod+alt+shift+p");
  });

  it("lowercases letters and symbols and maps Escape to esc", () => {
    expect(parseCombo(keyEvent({ key: "K", metaKey: true }))).toBe("mod+k");
    expect(parseCombo(keyEvent({ key: "/" }))).toBe("/");
    expect(parseCombo(keyEvent({ key: "/" , metaKey: true }))).toBe("mod+/");
    expect(parseCombo(keyEvent({ key: "Escape" }))).toBe("esc");
    expect(parseCombo(keyEvent({ key: "ArrowDown" }))).toBe("arrowdown");
  });

  it("ignores bare modifier presses", () => {
    expect(parseCombo(keyEvent({ key: "Meta", metaKey: true }))).toBeNull();
    expect(parseCombo(keyEvent({ key: "Shift", shiftKey: true }))).toBeNull();
  });
});

describe("formatCombo", () => {
  it("renders macOS glyph badges without separators", () => {
    expect(formatCombo("mod+k", true)).toBe("⌘K");
    expect(formatCombo("mod+shift+p", true)).toBe("⌘⇧P");
    expect(formatCombo("mod+/", true)).toBe("⌘/");
    expect(formatCombo("esc", true)).toBe("esc");
  });

  it("renders Windows/Linux textual badges with separators", () => {
    expect(formatCombo("mod+k", false)).toBe("Ctrl+K");
    expect(formatCombo("mod+alt+shift+p", false)).toBe("Ctrl+Alt+Shift+P");
  });
});

describe("shouldIgnore", () => {
  const input = { tagName: "INPUT" };
  const textarea = { tagName: "TEXTAREA" };
  const editable = { tagName: "DIV", isContentEditable: true };
  const button = { tagName: "BUTTON" };

  it("ignores plain keys typed inside editable targets", () => {
    expect(shouldIgnore(input, "k")).toBe(true);
    expect(shouldIgnore(textarea, "enter")).toBe(true);
    expect(shouldIgnore(editable, "x")).toBe(true);
  });

  it("lets mod combos through even inside editable targets", () => {
    expect(shouldIgnore(input, "mod+k")).toBe(false);
    expect(shouldIgnore(textarea, "mod+shift+p")).toBe(false);
  });

  it("never ignores keys outside editable targets", () => {
    expect(shouldIgnore(button, "k")).toBe(false);
    expect(shouldIgnore(null, "esc")).toBe(false);
    expect(shouldIgnore(undefined, "mod+/")).toBe(false);
  });
});
