// @vitest-environment jsdom
//! 456 号：AntiAiAndExperiencePanel 交互测试——规则新增合并/停用切换/经验沉淀
//! 三条 CRUD 链载荷断言（该面板状态自持无陈旧 props 风险，测试目的为锁定
//! 合并与切换语义）。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AntiAiAndExperiencePanel } from "./AntiAiAndExperiencePanel";
import { resetEngineKindCache } from "../hooks/use-engine-kind";

let rulesState = [
  { id: "seed_1", type: "phrase", pattern: "眼中闪过一丝", isRegex: false, severity: "warning", message: "高频 AI 神态套语", enabled: true },
] as Array<{ id: string; type: string; pattern: string; isRegex: boolean; severity: string; message: string; enabled: boolean }>;

const fetchJsonMock = vi.fn(async (path: string, init?: { method?: string; body?: string }) => {
  if (path.includes("/anti-ai-rules")) {
    if (init?.method === "PUT") {
      const payload = JSON.parse(init.body ?? "{}");
      rulesState = payload.rules;
      return { rules: rulesState };
    }
    return { rules: rulesState, seeded: true };
  }
  if (path.includes("/experience") && init?.method === "PUT") {
    const payload = JSON.parse(init.body ?? "{}");
    return { entries: payload.entries };
  }
  if (path.includes("/experience")) {
    return { entries: [] };
  }
  return {};
});

vi.mock("../hooks/use-api", () => ({
  fetchJson: (path: string, init?: { method?: string; body?: string }) => fetchJsonMock(path, init),
}));

afterEach(() => {
  cleanup();
  fetchJsonMock.mockClear();
  vi.unstubAllGlobals();
  resetEngineKindCache();
  rulesState = [
    { id: "seed_1", type: "phrase", pattern: "眼中闪过一丝", isRegex: false, severity: "warning", message: "高频 AI 神态套语", enabled: true },
  ];
});

/** 腿型桩：health 404 = Node 回退腿；200 = 产品引擎。 */
function stubEngine(healthStatus: number) {
  vi.stubGlobal("fetch", vi.fn(async () => ({ ok: healthStatus === 200, status: healthStatus }) as Response));
}

describe("AntiAiAndExperiencePanel 交互（456 号）", () => {
  it("种子徽标与规则列表渲染", async () => {
    render(<AntiAiAndExperiencePanel bookId="b1" />);
    expect(await screen.findByText("内置种子（未落盘，保存后生效为本书规则）")).toBeTruthy();
    expect(screen.getByText("眼中闪过一丝")).toBeTruthy();
  });

  it("新增规则合并保存——PUT 载荷含种子与新增项且保留原 enabled", async () => {
    const user = userEvent.setup();
    render(<AntiAiAndExperiencePanel bookId="b1" />);
    await user.type(screen.getByPlaceholderText("匹配模式（文本或正则）"), "嘴角勾起一抹");
    await user.type(screen.getByPlaceholderText("告警文案"), "高频表情模板");
    await user.click(screen.getByText("存规则"));

    await vi.waitFor(() => {
      expect(screen.getByText("规则已保存")).toBeTruthy();
    });
    const putCall = fetchJsonMock.mock.calls.find(
      ([path, init]) => path.includes("/anti-ai-rules") && (init as { method?: string })?.method === "PUT",
    );
    expect(putCall).toBeDefined();
    const payload = JSON.parse((putCall![1] as { body: string }).body);
    expect(payload.rules).toHaveLength(2);
    expect(payload.rules[0].pattern).toBe("眼中闪过一丝");
    expect(payload.rules[1].pattern).toBe("嘴角勾起一抹");
    // 纯中文模式无 slug 字符 → id 走 rule_<时间戳> 兜底（saveRule 派生逻辑）
    expect(payload.rules[1].id).toMatch(/^rule_\d+$/);
  });

  it("停用切换——PUT 载荷 enabled 翻转且其余规则保持", async () => {
    const user = userEvent.setup();
    render(<AntiAiAndExperiencePanel bookId="b1" />);
    await vi.waitFor(() => {
      expect(screen.getByText("停用")).toBeTruthy();
    });
    await user.click(screen.getByText("停用"));
    await vi.waitFor(() => {
      const putCall = fetchJsonMock.mock.calls.find(
        ([path, init]) => path.includes("/anti-ai-rules") && (init as { method?: string })?.method === "PUT",
      );
      expect(putCall).toBeDefined();
      const payload = JSON.parse((putCall![1] as { body: string }).body);
      expect(payload.rules[0].enabled).toBe(false);
    });
  });

  it("经验沉淀——PUT 载荷含条目文本且通知提示去重语义", async () => {
    const user = userEvent.setup();
    render(<AntiAiAndExperiencePanel bookId="b1" />);
    const input = screen.getByPlaceholderText("沉淀一条有效手法…");
    await user.type(input, "用镜面倒影切换视角制造悬念");
    await user.click(screen.getByText("沉淀"));

    expect(await screen.findByText("经验已沉淀（同文本自动去重）")).toBeTruthy();
    const putCall = fetchJsonMock.mock.calls.find(
      ([path, init]) => path.includes("/experience") && (init as { method?: string })?.method === "PUT",
    );
    expect(putCall).toBeDefined();
    const payload = JSON.parse((putCall![1] as { body: string }).body);
    expect(payload.entries[0].text).toBe("用镜面倒影切换视角制造悬念");
  });

  // 1169 号：IME 组合期回车（选词上屏）不触发沉淀——否则半截组合文本经
  // merge PUT 永久写入 experience.json；普通回车照常沉淀。
  it("IME 组合期回车不沉淀，普通回车沉淀（1169 号）", async () => {
    const user = userEvent.setup();
    render(<AntiAiAndExperiencePanel bookId="b1" />);
    const input = screen.getByPlaceholderText("沉淀一条有效手法…");
    await user.type(input, "以静写动");
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    const experiencePuts = () =>
      fetchJsonMock.mock.calls.filter(
        ([path, init]) => String(path).includes("/experience") && (init as { method?: string })?.method === "PUT",
      );
    expect(experiencePuts()).toHaveLength(0);
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText("经验已沉淀（同文本自动去重）")).toBeTruthy();
    expect(experiencePuts()).toHaveLength(1);
  });

  // 1668 号：规则禁则注入 / 审查扫描 / 经验注入的执行方只在产品引擎管线——
  // Node 回退腿上保存后不参与写作，面板如实标明；产品引擎在位不标。
  it("Node 回退腿——面板标明保存内容不参与写作与审查（1668 号）", async () => {
    stubEngine(404);
    render(<AntiAiAndExperiencePanel bookId="b1" />);
    const note = await screen.findByRole("note");
    expect(note.textContent).toContain("反AI规则与经验记忆由产品引擎执行");
    expect(note.textContent).toContain("Node 回退服务");
  });

  it("产品引擎在位——不标不生效（功能真在跑）", async () => {
    stubEngine(200);
    render(<AntiAiAndExperiencePanel bookId="b1" />);
    expect(await screen.findByText("内置种子（未落盘，保存后生效为本书规则）")).toBeTruthy();
    await act(async () => {});
    expect(screen.queryByRole("note")).toBeNull();
  });
});
