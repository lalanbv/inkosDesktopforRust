// @vitest-environment jsdom
// 1469：恢复沿行为测试——1464/1465 浏览器活体只终证 down 沿（横幅出现），
// 恢复沿（横幅消失+toast）在浏览器密集轮询中 toast 未捕获，此测试以
// 真 hook+真组件+可控 fetch 定谳恢复沿是否工作。
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import { EngineStatusBanner } from "@/components/EngineStatusBanner";

let fetchAlive = true;

describe("EngineStatusBanner recovery edge (1469)", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("down edge shows banner, recovery edge clears banner and shows toast", async () => {
    vi.useFakeTimers();
    const fetchMock = vi.fn(async () => ({ ok: fetchAlive }));
    vi.stubGlobal("fetch", fetchMock);

    // 死亡窗：首探 2s + 提频 2s×2 = 6s 判 down。
    fetchAlive = false;
    const { queryByText } = render(<EngineStatusBanner />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(6_200);
    });
    expect(queryByText("引擎连接中断")).not.toBeNull();

    // 恢复沿：down 态 2s 轮询捕获 → 横幅消失 + 恢复 toast。
    fetchAlive = true;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_200);
    });
    expect(queryByText("引擎连接中断")).toBeNull();
    expect(queryByText("引擎连接已恢复")).not.toBeNull();

    // toast 3s 自消。
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3_100);
    });
    expect(queryByText("引擎连接已恢复")).toBeNull();
  });
});
