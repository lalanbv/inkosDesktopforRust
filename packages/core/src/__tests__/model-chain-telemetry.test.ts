import { describe, expect, it } from "vitest";
import { InMemoryTelemetryContext } from "@earendil-works/pi-telemetry";
import { runWithModelChain } from "../llm/model-chain.js";
import { INKOS_AI_REQUEST_SPAN } from "../telemetry/inkos-ai-request.js";
import type { ResolvedTaskModelChain } from "../models/task-routing.js";

/**
 * R33 执行期接线测试（552 号）：model-chain 每次 attempt×round 发一个
 * `inkos.ai.request` span（粒度=RunLogEntry 一一对应）——成功 status=ok、
 * 失败 errorKind 进属性且 status=error、异常正确上抛、NOOP 缺省零行为
 * （既有 model-chain.test 全绿即证）。
 */

const CHAIN: ResolvedTaskModelChain = {
  attempts: [
    { model: "primary" },
    { model: "backup-a" },
    { model: "backup-b" },
  ],
  retryCount: 2,
} as never;

/** 依次返回既定结果/错误的 flaky 序列（类 483 号 flaky mock 形态）。 */
function flakyRunner(script: Array<{ ok: boolean; error?: unknown; value?: string }>) {
  let index = 0;
  const calls: Array<{ model: string; attempt?: { attemptIndex: number; round: number; label: string } }> = [];
  return {
    calls,
    run: async (model: string, attempt?: { attemptIndex: number; round: number; label: string }) => {
      calls.push({ model, attempt });
      const step = script[index++];
      if (step === undefined) throw new Error("flaky script exhausted");
      if (!step.ok) throw step.error;
      return step.value ?? "ok";
    },
  };
}

const transient503 = Object.assign(new Error("503 upstream temporarily unavailable"), { status: 503 });

describe("model-chain × inkos.ai.request span（R33 执行期接线）", () => {
  it("emits one span per attempt×round with chain semantics on success", async () => {
    const telemetry = new InMemoryTelemetryContext();
    // primary 第 1 轮成功——单 span、ok、attempt 语义归零。
    const runner = flakyRunner([{ ok: true, value: "done" }]);
    const result = await runWithModelChain(runner.run, {
      chain: CHAIN,
      primaryModel: "primary",
      label: "writer",
      telemetry,
    });
    expect(result).toBe("done");
    const spans = telemetry.getSpans();
    expect(spans).toHaveLength(1);
    expect(spans[0]!.name).toBe(INKOS_AI_REQUEST_SPAN);
    expect(spans[0]!.attributes).toMatchObject({
      "pi.ai.model": "primary",
      "inkos.agent": "writer",
      "inkos.attempt_index": 0,
      "inkos.round": 1,
      "inkos.took_over": false,
    });
    expect(spans[0]!.status).toEqual({ status: "ok" });
  });

  it("records transient errorKind on failed spans and continues the chain", async () => {
    const telemetry = new InMemoryTelemetryContext();
    // primary 两轮全瞬态失败 → backup-a 首轮接管成功。
    const runner = flakyRunner([
      { ok: false, error: transient503 },
      { ok: false, error: transient503 },
      { ok: true, value: "rescued" },
    ]);
    const result = await runWithModelChain(runner.run, {
      chain: CHAIN,
      primaryModel: "primary",
      label: "planner",
      telemetry,
    });
    expect(result).toBe("rescued");
    const spans = telemetry.getSpans();
    expect(spans).toHaveLength(3);

    expect(spans[0]!.attributes["inkos.attempt_index"]).toBe(0);
    expect(spans[0]!.attributes["inkos.round"]).toBe(1);
    expect(spans[0]!.attributes["inkos.error_kind"]).toBe("transient");
    expect(spans[0]!.status.status).toBe("error");

    expect(spans[1]!.attributes["inkos.round"]).toBe(2);
    expect(spans[1]!.attributes["inkos.error_kind"]).toBe("transient");

    // 接管 span：attemptIndex=1、took_over=true、status ok。
    expect(spans[2]!.attributes["pi.ai.model"]).toBe("backup-a");
    expect(spans[2]!.attributes["inkos.attempt_index"]).toBe(1);
    expect(spans[2]!.attributes["inkos.took_over"]).toBe(true);
    expect(spans[2]!.status).toEqual({ status: "ok" });

    // run 回调第二参透出 attempt 语义（552 号签名扩展，闭包可选择性接收）。
    expect(runner.calls[2]!.attempt).toMatchObject({ attemptIndex: 1, round: 1, label: "planner" });
  });

  it("marks fatal errorKind on non-retryable failures (no chain continuation)", async () => {
    const telemetry = new InMemoryTelemetryContext();
    const fatal = Object.assign(new Error("401 unauthorized"), { status: 401 });
    const runner = flakyRunner([{ ok: false, error: fatal }]);
    await expect(
      runWithModelChain(runner.run, {
        chain: CHAIN,
        primaryModel: "primary",
        label: "auditor",
        telemetry,
      }),
    ).rejects.toThrow("401 unauthorized");
    const spans = telemetry.getSpans();
    expect(spans).toHaveLength(1);
    expect(spans[0]!.attributes["inkos.error_kind"]).toBe("fatal");
    expect(spans[0]!.status.status).toBe("error");
  });
});
