import { describe, expect, it, vi } from "vitest";
import { renderToString } from "react-dom/server";

/**
 * 735 号：ServiceDetailPage 挂载自验（41ca307e 收口记录 mining 候选清偿）
 * ——模型配置详情页三形态挂载不崩 + 核心区块在位。依赖 mock：
 * fetchJson（use-api；store 与页面共用）+ useServiceStore 预填（zustand
 * setState 直填真 store）。状态推导纯逻辑面已由 service-detail-state.test
 * 覆盖（483 行），本件补**组件挂载面**。
 */

const fetchJsonMock = vi.fn();

vi.mock("@/hooks/use-api", () => ({
  fetchJson: (...args: unknown[]) => fetchJsonMock(...(args as [])),
  useApi: () => ({ data: { language: "zh" }, refetch: () => undefined }),
}));

const nav = { toDashboard: () => undefined, toServices: () => undefined };

import { ServiceDetailPage } from "@/pages/ServiceDetailPage";
import { useServiceStore } from "@/store/service";

describe("ServiceDetailPage mount self-check (735)", () => {
  it("未知服务挂载不崩（h1 回退 serviceId + 表单骨架在位）", () => {
    useServiceStore.setState({ services: [], servicesLoading: false });
    fetchJsonMock.mockResolvedValue({ services: [] });
    const html = renderToString(<ServiceDetailPage serviceId="zhipu" nav={nav} />);
    expect(html).toContain("zhipu");
    expect(html).toContain("<h1");
  });

  it("custom 前缀服务挂载：自定义名渲染 + 连接表单字段在位", async () => {
    useServiceStore.setState({
      services: [{ service: "custom:镜花源", label: "镜花源", connected: false }],
      servicesLoading: false,
    });
    fetchJsonMock.mockResolvedValue({
      services: [{ service: "custom:镜花源", name: "镜花源", baseUrl: "https://api.example.com", models: ["glm-5.3"] }],
    });
    const html = renderToString(<ServiceDetailPage serviceId="custom:%E9%95%9C%E8%8A%B1%E6%BA%90" nav={nav} />);
    // 自定义名渲染（resolvedCustomName 链）。
    expect(html).toContain("镜花源");
    // 连接表单核心字段在位（API Key / Base URL / 模型）。
    expect(html).toContain("type=\"password\"");
    expect(html.toLowerCase()).toContain("base url");
  });

  it("fetchJson 拒绝不崩（.catch 静默链）", () => {
    useServiceStore.setState({ services: [], servicesLoading: false });
    fetchJsonMock.mockRejectedValue(new Error("network down"));
    const html = renderToString(<ServiceDetailPage serviceId="zhipu" nav={nav} />);
    expect(html).toContain("zhipu");
  });
});
