import { useRef } from "react";
import type { SSEMessage } from "./use-sse";
import { useNewSSEMessages } from "./use-sse";

/**
 * R44/566 号：订阅全局 SSE 流中的 `skills:change` 失效事件，触发技能列表
 * 重取（ChatPage 技能选择器 / ProjectSettings 技能库）。发射点=author_skill
 * 写链（R37）成功、skills import、skills delete——UI 打开中的面板即时热刷新。
 * 复用 App 的单一 SSE 流（sse prop 传入），不另开 EventSource 连接。
 */
export function useSkillsInvalidation(
  sse: { messages: ReadonlyArray<SSEMessage> },
  invalidate: () => void,
): void {
  const invalidateRef = useRef(invalidate);
  invalidateRef.current = invalidate;
  useNewSSEMessages(sse.messages, (recent) => {
    if (recent.event === "skills:change") {
      invalidateRef.current();
    }
  });
}
