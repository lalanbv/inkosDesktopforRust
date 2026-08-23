import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { PaletteBody } from "@/components/CommandPalette";
import { buildActionCommands, buildNavigationCommands } from "@/lib/commands";
import type { CommandEntry } from "@/lib/commands";

const noop = () => {};

describe("PaletteBody", () => {
  const nav = buildNavigationCommands();
  const actions = buildActionCommands();
  const fallback = actions.find((entry) => entry.id === "action.bookCreate")!;

  const render = (groups: ReadonlyArray<{ key: string; label: string; entries: CommandEntry[] }>) =>
    renderToString(
      <PaletteBody
        groups={groups}
        lang="zh"
        fallbackEntry={fallback}
        onRun={noop}
        query=""
        onQueryChange={noop}
      />,
    );

  it("renders grouped entries with their titles", () => {
    const html = render([
      { key: "navigation", label: "前往", entries: nav.slice(0, 3) },
      { key: "action", label: "操作", entries: actions.slice(0, 2) },
    ]);
    expect(html).toContain("前往");
    expect(html).toContain("操作");
    expect(html).toContain(nav[0].titleZh);
    expect(html).toContain(actions[0].titleZh);
  });

  it("shows the English title when the UI language is English", () => {
    const html = renderToString(
      <PaletteBody
        groups={[{ key: "navigation", label: "Go to", entries: nav.slice(0, 1) }]}
        lang="en"
        fallbackEntry={fallback}
        onRun={noop}
        query=""
        onQueryChange={noop}
      />,
    );
    expect(html).toContain(nav[0].titleEn);
  });

  it("falls back to a create-book action instead of a dead end when nothing matches", () => {
    const html = render([{ key: "navigation", label: "前往", entries: [] }]);
    expect(html).toContain('data-slot="palette-empty"');
    expect(html).toContain("没有匹配的命令");
    expect(html).toContain("创建新书");
  });

  it("skips empty groups entirely when other groups have entries", () => {
    const html = render([
      { key: "recent", label: "最近访问", entries: [] },
      { key: "navigation", label: "前往", entries: nav.slice(0, 2) },
    ]);
    expect(html).not.toContain("最近访问");
    expect(html).toContain("前往");
    expect(html).not.toContain('data-slot="palette-empty"');
  });
});
