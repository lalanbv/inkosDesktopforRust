import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { StatusBar } from "@/components/StatusBar";

const baseProps = {
  sseConnected: true,
  onReconnect: () => {},
  daemonRunning: true,
  onOpenBottomPanel: () => {},
};

describe("StatusBar workspace switcher (869)", () => {
  it("renders four workspace buttons when a switch handler is provided", () => {
    const html = renderToString(
      <StatusBar {...baseProps} workspaceActiveId="writing" onSwitchWorkspace={() => {}} />,
    );
    expect(html).toContain('data-testid="workspace-switcher"');
    expect(html).toContain('data-testid="workspace-writing"');
    expect(html).toContain('data-testid="workspace-review"');
    expect(html).toContain('data-testid="workspace-analysis"');
    expect(html).toContain('data-testid="workspace-film"');
  });

  it("marks the active workspace with aria-pressed", () => {
    const html = renderToString(
      <StatusBar {...baseProps} workspaceActiveId="review" onSwitchWorkspace={() => {}} />,
    );
    expect(html).toContain('data-testid="workspace-review"');
    // 写作未激活不应带 aria-pressed="true"；激活的工作区应带
    expect(html.match(/aria-pressed="true"/g)?.length).toBe(1);
    expect(html).toContain('title="审阅工作区"');
  });

  it("never flags an active workspace before the first switch (null activeId)", () => {
    const html = renderToString(
      <StatusBar {...baseProps} workspaceActiveId={null} onSwitchWorkspace={() => {}} />,
    );
    expect(html).not.toContain('aria-pressed="true"');
  });

  it("omits the switcher entirely without a handler (standalone/test usage)", () => {
    const html = renderToString(<StatusBar {...baseProps} />);
    expect(html).not.toContain('data-testid="workspace-switcher"');
  });
});
