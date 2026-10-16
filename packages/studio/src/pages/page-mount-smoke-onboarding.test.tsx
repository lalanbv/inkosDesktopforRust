// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

/**
 * 739 号附：OnboardingWizard 挂载冒烟——组件调 isOnboardingComplete()
 * （默认 window.localStorage），须 jsdom 环境（node 环境下 window undefined）。
 */

const useApiMock = vi.fn();
const fetchJsonMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
}));

const nav = { toDashboard: () => undefined } as never;
const theme = "light" as const;

import { OnboardingWizard } from "@/pages/OnboardingWizard";

describe("OnboardingWizard mount smoke (739 附)", () => {
  it("新手向导挂载（jsdom 环境；向导标题在位）", () => {
    useApiMock.mockImplementation(() => ({ data: { llmConnected: false }, refetch: () => undefined }));
    fetchJsonMock.mockResolvedValue({});
    const html = renderToString(<OnboardingWizard nav={nav} theme={theme} />);
    expect(html).toContain("onboarding-wizard");
    expect(html).toContain("新手创作向导");
  });
});
