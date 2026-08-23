import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { LanguageOptions } from "@/pages/LanguageSelector";

describe("LanguageOptions", () => {
  it("renders both language options", () => {
    const html = renderToString(<LanguageOptions selected={null} onSelect={() => {}} />);
    expect(html).toContain('data-language="zh"');
    expect(html).toContain('data-language="en"');
    expect(html).toContain("中文创作");
    expect(html).toContain("English Writing");
  });

  it("marks itself as the language selector slot for e2e targeting", () => {
    const html = renderToString(<LanguageOptions selected={null} onSelect={() => {}} />);
    expect(html).toContain('data-slot="language-selector"');
  });
});
