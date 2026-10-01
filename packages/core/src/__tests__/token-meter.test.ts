import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { TokenMeter } from "../utils/token-meter.js";

/**
 * R41 token 计量双端共享 golden 回放（563/564 号）：与 Rust 侧
 * `engine-rs/tests/golden_token_meter_diff.rs` 消费同一份向量——
 * 锚点三态（usage 锚/估算/收缩拒绝）、clamp、覆盖率、超窗旗标码点级锁死。
 * 改向量必须双端同批。
 */

const VECTORS: {
  version: number;
  cases: Array<{
    name: string;
    inputWindow: number;
    ops: Array<
      | { op: "append"; text: string; repeat?: number }
      | {
          op: "noteUsage";
          model: string | null;
          promptTokens: number;
          completionTokens: number;
          totalTokens: number;
          accepted: boolean;
        }
      | { op: "measure"; expect: Record<string, unknown> }
    >;
  }>;
} = JSON.parse(
  readFileSync(new URL("./golden/token-meter-vectors.json", import.meta.url), "utf8"),
);

describe("token meter shared golden vectors (R41 / 563+564)", () => {
  it("loads the shared vector file", () => {
    expect(VECTORS.version).toBe(1);
    expect(VECTORS.cases.length).toBeGreaterThan(0);
  });

  for (const testCase of VECTORS.cases) {
    it(`replays ${testCase.name}`, () => {
      const meter = new TokenMeter(testCase.inputWindow);
      for (const op of testCase.ops) {
        if (op.op === "append") {
          const repeat = op.repeat ?? 1;
          for (let i = 0; i < repeat; i++) {
            meter.append(op.text);
          }
        } else if (op.op === "noteUsage") {
          const accepted = meter.noteUsage(op.model, op.promptTokens, op.completionTokens, op.totalTokens);
          expect(accepted).toBe(op.accepted);
        } else {
          expect(meter.measure()).toEqual(op.expect);
        }
      }
    });
  }
});
