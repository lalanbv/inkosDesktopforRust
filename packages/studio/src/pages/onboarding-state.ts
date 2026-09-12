/**
 * R9/370 号：新手创作向导状态机（纯函数，可测）。
 *
 * 首跑三步：厂商 → Key → 能力探测。步骤勾选读真实状态：
 * - 已配置服务（services/config 有条目）→ 步骤 1 完成；
 * - 选中服务的 secret 已存在 → 步骤 2 完成；
 * - doctor llmConnected 或本次探测通过 → 步骤 3 完成（AI 功能放行）。
 */

export type OnboardingStepId = "provider" | "key" | "probe";

export interface OnboardingStep {
  readonly id: OnboardingStepId;
  readonly done: boolean;
}

export interface OnboardingInput {
  /** 已配置服务 id 集合（services/config.services）。 */
  readonly configuredServiceIds: ReadonlyArray<string>;
  /** 向导中选中的服务 id（可为空）。 */
  readonly selectedServiceId?: string;
  /** 选中服务的 secret 是否已存在。 */
  readonly selectedServiceHasSecret: boolean;
  /** 既有探测结论（doctor llmConnected）。 */
  readonly llmConnected: boolean;
  /** 本次向导内探测通过（session 级）。 */
  readonly probedOk?: boolean;
}

export function computeOnboardingSteps(input: OnboardingInput): ReadonlyArray<OnboardingStep> {
  const providerDone = input.configuredServiceIds.length > 0 || Boolean(input.selectedServiceId);
  const keyDone =
    Boolean(input.selectedServiceId) &&
    (input.selectedServiceHasSecret || input.probedOk === true);
  const probeDone = input.llmConnected || input.probedOk === true;
  return [
    { id: "provider", done: providerDone },
    { id: "key", done: keyDone },
    { id: "probe", done: probeDone },
  ];
}

/**
 * AI 功能放行判定：探测步骤（probe.done）为唯一硬条件——
 * doctor llmConnected 或本次探测通过即放行；厂商/Key 步骤为引导性进度。
 */
export function isOnboardingUnlocked(steps: ReadonlyArray<OnboardingStep>): boolean {
  return steps.find((step) => step.id === "probe")?.done ?? false;
}

// ── 完成标记（localStorage，可注入便于测试）──

const STORAGE_KEY = "inkos.onboarding.complete";

export function isOnboardingComplete(storage: Pick<Storage, "getItem"> = window.localStorage): boolean {
  try {
    return storage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

export function markOnboardingComplete(storage: Pick<Storage, "setItem"> = window.localStorage): void {
  try {
    storage.setItem(STORAGE_KEY, "1");
  } catch {
    // 隐私模式等存储不可用——静默忽略（放行判定以后端探测为准）。
  }
}
