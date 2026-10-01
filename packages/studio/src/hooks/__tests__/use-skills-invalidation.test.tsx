// @vitest-environment jsdom
//! R44/566 号：useSkillsInvalidation 交互测试——skills:change 触发重取、
//! 其他事件不触发、同批消息 React effect 复跑不重复触发（游标消费语义）。
import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useSkillsInvalidation } from "../use-skills-invalidation";
import type { SSEMessage } from "../use-sse";

function Probe({ messages, onInvalidate }: { messages: ReadonlyArray<SSEMessage>; onInvalidate: () => void }) {
  useSkillsInvalidation({ messages }, onInvalidate);
  return null;
}

describe("useSkillsInvalidation (R44 / 566)", () => {
  afterEach(cleanup);

  it("fires invalidate on skills:change and ignores other events", () => {
    const onInvalidate = vi.fn();
    const { rerender } = render(<Probe messages={[]} onInvalidate={onInvalidate} />);

    const other: SSEMessage[] = [
      { event: "agent:complete", data: null, timestamp: 1, seq: 1 },
    ];
    rerender(<Probe messages={other} onInvalidate={onInvalidate} />);
    expect(onInvalidate).not.toHaveBeenCalled();

    const hit: SSEMessage[] = [
      ...other,
      { event: "skills:change", data: { reason: "authored" }, timestamp: 2, seq: 2 },
    ];
    rerender(<Probe messages={hit} onInvalidate={onInvalidate} />);
    expect(onInvalidate).toHaveBeenCalledTimes(1);

    // 同批消息重放（React effect 复跑）不重复触发——游标消费。
    rerender(<Probe messages={hit} onInvalidate={onInvalidate} />);
    expect(onInvalidate).toHaveBeenCalledTimes(1);

    const second: SSEMessage[] = [
      ...hit,
      { event: "skills:change", data: { reason: "deleted" }, timestamp: 3, seq: 3 },
    ];
    rerender(<Probe messages={second} onInvalidate={onInvalidate} />);
    expect(onInvalidate).toHaveBeenCalledTimes(2);
  });
});
