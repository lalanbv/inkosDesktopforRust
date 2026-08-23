import { describe, expect, it } from "vitest";
import {
  TITLEBAR_INSET_PX,
  deriveHeaderInsetClass,
  isMacPlatformAgent,
  isTauriRuntime,
} from "@/lib/titlebar";

describe("isMacPlatformAgent", () => {
  it("matches Mac platform strings and rejects others", () => {
    expect(isMacPlatformAgent("MacIntel")).toBe(true);
    expect(isMacPlatformAgent("Mac ARM64")).toBe(true);
    expect(isMacPlatformAgent("Win32")).toBe(false);
    expect(isMacPlatformAgent("Linux x86_64")).toBe(false);
    expect(isMacPlatformAgent("")).toBe(false);
  });
});

describe("isTauriRuntime", () => {
  it("detects the Tauri v2 injected internals global", () => {
    expect(isTauriRuntime({ __TAURI_INTERNALS__: { invoke: () => {} } })).toBe(true);
  });

  it("rejects plain globals, null and undefined", () => {
    expect(isTauriRuntime({})).toBe(false);
    expect(isTauriRuntime(null)).toBe(false);
    expect(isTauriRuntime(undefined)).toBe(false);
  });
});

describe("deriveHeaderInsetClass", () => {
  it("reserves traffic-light inset only inside the macOS Tauri shell", () => {
    expect(deriveHeaderInsetClass(true, true)).toBe("pl-[78px]");
    // 浏览器开发（非 Tauri 壳）与 Win/Linux 系统标题栏不缩进
    expect(deriveHeaderInsetClass(true, false)).toBe("pl-8");
    expect(deriveHeaderInsetClass(false, true)).toBe("pl-8");
    expect(deriveHeaderInsetClass(false, false)).toBe("pl-8");
  });

  it("keeps the inset class and the constant in sync", () => {
    expect(deriveHeaderInsetClass(true, true)).toBe(`pl-[${TITLEBAR_INSET_PX}px]`);
  });
});
