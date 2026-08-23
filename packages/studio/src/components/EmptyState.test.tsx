import { describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { EmptyState } from "@/components/EmptyState";
import { BookOpen, Film } from "lucide-react";

describe("EmptyState", () => {
  it("renders the title, description and action button", () => {
    const html = renderToString(
      <EmptyState
        icon={<BookOpen size={28} />}
        title="还没有书"
        description="创建第一本书开始写作"
        actionLabel="新建书籍"
        onAction={() => {}}
      />,
    );
    expect(html).toContain("还没有书");
    expect(html).toContain("创建第一本书开始写作");
    expect(html).toContain("新建书籍");
    expect(html).toContain('data-slot="empty-state-action"');
  });

  it("omits the action button when no action is provided", () => {
    const html = renderToString(
      <EmptyState icon={<Film size={28} />} title="还没有会话" />,
    );
    expect(html).toContain("还没有会话");
    expect(html).not.toContain('data-slot="empty-state-action"');
  });
});
