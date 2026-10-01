// @vitest-environment jsdom
//! 575 号：服务商列表「自定义服务」区块置顶——574 号走查备案的三层深引导链
//! （配置模型 → 服务商列表 → 滚动到底找自定义）收敛为两层：零配置新用户
//! 从聊天页「配置模型 →」进入后零滚动直达自定义服务表单入口。
//! 断言=自定义 heading 在第一个 preset 分组 heading 之前（DOM 序）。
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ServiceListPage } from "../ServiceListPage";
import { useServiceStore } from "../../store/service";

vi.mock("../hooks/use-api", () => ({
  fetchJson: vi.fn().mockResolvedValue([]),
}));

function seedStore() {
  useServiceStore.setState({
    services: [
      { service: "kkaiapi", label: "kkaiapi", group: "aggregator", connected: false },
      { service: "custom:Mock", label: "Mock", connected: true },
    ],
    servicesLoading: false,
  } as never);
}

const navStub = {
  toServiceDetail: vi.fn(),
} as never;

describe("ServiceListPage custom-first ordering (575)", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders the custom-services section before the first preset group", () => {
    seedStore();
    const { container } = render(<ServiceListPage nav={navStub} />);

    const customHeading = screen.getByRole("heading", { name: "自定义服务" });
    const firstGroupHeading = screen.getByText("聚合 API");
    // DOM 序：自定义区块必须先于任何 preset 分组（置顶语义的机械断言）。
    expect(
      customHeading.compareDocumentPosition(firstGroupHeading) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    // 自定义新建入口（虚线卡）在场且指向 custom 表单。
    const createCard = screen.getByRole("button", { name: "自定义服务" });
    expect(createCard).toBeTruthy();
    expect(container.textContent).toContain("kkaiapi");
  });
});
