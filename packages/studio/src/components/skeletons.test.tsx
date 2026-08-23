import { describe, expect, it, vi, afterEach } from "vitest";
import { renderToString } from "react-dom/server";
import { Skeleton } from "@/components/ui/skeleton";
import {
  SKELETON_DELAY_MS,
  SkeletonCards,
  SkeletonParagraphs,
  SkeletonRows,
  scheduleDelayedReveal,
} from "@/components/skeletons";

function occurrences(html: string, needle: string): number {
  return html.split(needle).length - 1;
}

describe("Skeleton base", () => {
  it("marks itself as a skeleton slot with the pulse animation", () => {
    const html = renderToString(<Skeleton className="h-4 w-10" />);
    expect(html).toContain('data-slot="skeleton"');
    expect(html).toContain("animate-pulse");
    expect(html).toContain("bg-muted");
    expect(html).toContain("h-4");
  });
});

describe("SkeletonRows", () => {
  it("renders one icon circle and two text bars per row with a busy status", () => {
    const html = renderToString(<SkeletonRows count={6} />);
    expect(html).toContain('data-slot="skeleton-rows"');
    expect(html).toContain('role="status"');
    expect(html).toContain('aria-busy="true"');
    expect(occurrences(html, "rounded-full")).toBe(6);
  });

  it("staggers row title widths so consecutive rows do not look copy-pasted", () => {
    const html = renderToString(<SkeletonRows count={4} />);
    expect(html).toContain("w-3/5");
    expect(html).toContain("w-2/5");
  });
});

describe("SkeletonCards", () => {
  it("renders the requested number of cards", () => {
    const html = renderToString(<SkeletonCards count={3} />);
    expect(html).toContain('data-slot="skeleton-cards"');
    expect(occurrences(html, 'data-slot="skeleton-card"')).toBe(3);
    expect(html).toContain("rounded-2xl");
  });
});

describe("SkeletonParagraphs", () => {
  it("renders paragraph blocks with staggered tail widths", () => {
    const html = renderToString(<SkeletonParagraphs count={4} />);
    expect(html).toContain('data-slot="skeleton-paragraphs"');
    expect(html).toContain("w-11/12");
    expect(html).toContain("w-3/4");
  });
});

describe("scheduleDelayedReveal", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("waits the full delay before revealing", () => {
    vi.useFakeTimers();
    const onReveal = vi.fn();
    scheduleDelayedReveal(SKELETON_DELAY_MS, onReveal);
    vi.advanceTimersByTime(SKELETON_DELAY_MS - 1);
    expect(onReveal).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onReveal).toHaveBeenCalledTimes(1);
  });

  it("cancel prevents the reveal and is idempotent", () => {
    vi.useFakeTimers();
    const onReveal = vi.fn();
    const cancel = scheduleDelayedReveal(SKELETON_DELAY_MS, onReveal);
    cancel();
    cancel();
    vi.advanceTimersByTime(SKELETON_DELAY_MS * 2);
    expect(onReveal).not.toHaveBeenCalled();
  });
});
