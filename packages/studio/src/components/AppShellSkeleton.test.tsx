import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { AppShellSkeleton } from "@/components/AppShellSkeleton";

function occurrences(html: string, needle: string): number {
  return html.split(needle).length - 1;
}

describe("AppShellSkeleton", () => {
  const html = renderToString(<AppShellSkeleton />);

  it("marks the whole shell as loading for e2e/perf probes", () => {
    expect(html).toContain('data-loading="shell"');
    expect(html).toContain('aria-busy="true"');
  });

  it("mirrors the ready layout: header, sidebar and main regions all present", () => {
    expect(html).toContain('data-slot="skeleton-header"');
    expect(html).toContain('data-slot="skeleton-sidebar"');
    expect(html).toContain('data-slot="skeleton-main"');
  });

  it("keeps the sidebar width identical to the real Sidebar to avoid a layout jump", () => {
    expect(html).toContain("w-[260px]");
  });

  it("fills the sidebar with 6 nav rows and the main area with 3 cards", () => {
    expect(occurrences(html, "rounded-full")).toBe(6);
    expect(occurrences(html, 'data-slot="skeleton-card"')).toBe(3);
  });

  it("breathes the logo with the shared glow animation instead of a spinner", () => {
    expect(html).toContain("chat-icon-glow");
    expect(html).not.toContain("animate-spin");
  });
});
