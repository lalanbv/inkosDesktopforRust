//! R9/370 号：新手向导状态机单测（步骤勾选读真实状态 + 完成标记存取）。
import { describe, expect, it } from "vitest";
import {
  computeOnboardingSteps,
  isOnboardingComplete,
  isOnboardingUnlocked,
  markOnboardingComplete,
} from "./onboarding-state";

describe("onboarding state (R9)", () => {
  it("fresh state has no steps done and stays locked", () => {
    const steps = computeOnboardingSteps({
      configuredServiceIds: [],
      selectedServiceHasSecret: false,
      llmConnected: false,
    });
    expect(steps.map((step) => step.done)).toEqual([false, false, false]);
    expect(isOnboardingUnlocked(steps)).toBe(false);
  });

  it("reads real state: configured services and secret mark provider/key done", () => {
    const steps = computeOnboardingSteps({
      configuredServiceIds: ["deepseek"],
      selectedServiceId: "deepseek",
      selectedServiceHasSecret: true,
      llmConnected: false,
    });
    expect(steps.map((step) => step.done)).toEqual([true, true, false]);
    expect(isOnboardingUnlocked(steps)).toBe(false);
  });

  it("probe pass or doctor llmConnected unlocks", () => {
    const probed = computeOnboardingSteps({
      configuredServiceIds: ["deepseek"],
      selectedServiceId: "deepseek",
      selectedServiceHasSecret: true,
      llmConnected: false,
      probedOk: true,
    });
    expect(isOnboardingUnlocked(probed)).toBe(true);

    const doctorConnected = computeOnboardingSteps({
      configuredServiceIds: [],
      selectedServiceHasSecret: false,
      llmConnected: true,
    });
    expect(isOnboardingUnlocked(doctorConnected)).toBe(true);
  });

  it("completion marker round-trips with injectable storage", () => {
    const store = new Map<string, string>();
    const storage = {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
    } as Pick<Storage, "getItem" | "setItem">;

    expect(isOnboardingComplete(storage)).toBe(false);
    markOnboardingComplete(storage);
    expect(isOnboardingComplete(storage)).toBe(true);
  });
});
