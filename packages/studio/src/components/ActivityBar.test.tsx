import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { ActivityBar } from "@/components/ActivityBar";

const nav = new Proxy({}, { get: () => () => {} }) as Record<string, () => void>;
const t = (key: string) => key;

describe("ActivityBar", () => {
  const html = renderToString(
    <ActivityBar nav={nav as never} activeSection="tools" onSelectSection={() => {}} t={t} lang="zh" />,
  );

  it("renders the four zone buttons plus the bottom settings entry", () => {
    expect(html).toContain('data-testid="activity-create"');
    expect(html).toContain('data-testid="activity-tools"');
    expect(html).toContain('data-testid="activity-manage"');
    expect(html).toContain('data-testid="activity-film"');
    expect(html).toContain('data-testid="activity-settings"');
  });

  it("marks the active zone with aria-current and bilingual titles", () => {
    expect(html).toContain('aria-current="page"');
    expect(html).toContain('title="工具"');
    expect(html).toContain('title="创作"');
    expect(html).toContain('title="互动影视"');
  });

  it("keeps the 48px rail width", () => {
    expect(html).toContain("w-12");
  });
});
