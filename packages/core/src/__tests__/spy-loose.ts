import { vi } from "vitest";
import type { MockInstance } from "vitest";

/**
 * 受保护/私有方法的 spyOn 统一出口（482 号）。
 *
 * vitest 5 收紧 spyOn 泛型后，历史惯用法 `vi.spyOn(X.prototype as never, "m" as never)`
 * 会把 spy 塌缩成 `never`（mockResolvedValue 等不可达，TS2339）。本助手牺牲方法
 * 签名精度换取可达性——返回宽松 MockInstance，mock 系方法与 mock.calls 全量可用。
 * 仅测试用：能走常规 vi.spyOn（public 方法）时不要用这里。
 */
export function spyOnLoose<T extends object>(
  object: T,
  method: string,
): MockInstance<(...args: never[]) => unknown> {
  return vi.spyOn(object as never, method as never);
}
