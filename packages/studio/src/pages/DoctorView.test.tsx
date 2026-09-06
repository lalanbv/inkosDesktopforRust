import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";
import { DoctorView } from "@/pages/DoctorView";

const useApiMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  useApi: (path: string) => useApiMock(path),
}));

const nav = { toDashboard: () => undefined };
const t = (key: string) => key;
const theme = "light" as const;

const BASE_CHECKS = {
  inkosJson: true,
  projectEnv: false,
  globalEnv: true,
  booksDir: true,
  llmConnected: true,
  bookCount: 2,
};

describe("DoctorView（195 号书籍级写作阻塞预警）", () => {
  it("有 state-degraded 问题书 → 渲染预警行与修复指引", () => {
    useApiMock.mockReturnValue({
      data: {
        ...BASE_CHECKS,
        bookIssues: [{ bookId: "b1", title: "测试书", kind: "state-degraded", chapter: 2 }],
      },
      refetch: vi.fn(),
    });
    const html = renderToString(<DoctorView nav={nav} theme={theme} t={t} />);
    expect(html).toContain('data-slot="doctor-book-issue"');
    expect(html).toContain("测试书");
    expect(html).toContain("doctor.issueStateDegraded");
    expect(html).not.toContain("doctor.bookHealthOk");
  });

  it("无问题书 → 书籍健康行显示通过；旧后端缺 bookIssues 字段同样按无问题渲染", () => {
    useApiMock.mockReturnValue({ data: { ...BASE_CHECKS, bookIssues: [] }, refetch: vi.fn() });
    const html = renderToString(<DoctorView nav={nav} theme={theme} t={t} />);
    expect(html).toContain('data-slot="doctor-book-health"');
    expect(html).toContain("doctor.bookHealthOk");

    useApiMock.mockReturnValue({ data: { ...BASE_CHECKS }, refetch: vi.fn() });
    const legacy = renderToString(<DoctorView nav={nav} theme={theme} t={t} />);
    expect(legacy).toContain("doctor.bookHealthOk");
    expect(legacy).not.toContain('data-slot="doctor-book-issue"');
  });
});
