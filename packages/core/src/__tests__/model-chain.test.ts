import { describe, expect, it } from "vitest";
import { runWithModelChain, type ModelChainEvent } from "../llm/model-chain.js";
import type { ResolvedTaskModelChain } from "../models/task-routing.js";

function chainOf(
  attempts: Array<{ model: string; service?: string }>,
  retryCount = 1,
): ResolvedTaskModelChain {
  return { task: "writing", attempts, retryCount };
}

const RETRYABLE = () => new Error("API 返回 429 (请求过多)。rate limit");
const FATAL = () => new Error("401 unauthorized: invalid api key");

describe("runWithModelChain (R25)", () => {
  it("passes through with primary model when no chain is attached", async () => {
    const calls: string[] = [];
    const result = await runWithModelChain(async (model) => {
      calls.push(model);
      return `ok:${model}`;
    }, { primaryModel: "main" });
    expect(result).toBe("ok:main");
    expect(calls).toEqual(["main"]);
  });

  it("passes through for single-attempt chain with retryCount 1", async () => {
    const calls: string[] = [];
    const result = await runWithModelChain(async (model) => {
      calls.push(model);
      return model;
    }, {
      primaryModel: "main",
      chain: chainOf([{ model: "main" }]),
    });
    expect(result).toBe("main");
    expect(calls).toEqual(["main"]);
  });

  it("fails over to the backup model on retryable errors", async () => {
    const calls: string[] = [];
    const events: ModelChainEvent[] = [];
    const result = await runWithModelChain(async (model) => {
      calls.push(model);
      if (model === "primary-m") throw RETRYABLE();
      return `ok:${model}`;
    }, {
      primaryModel: "primary-m",
      label: "writer",
      chain: chainOf([{ model: "primary-m" }, { model: "backup-m" }]),
      onEvent: (event) => events.push(event),
    });
    expect(result).toBe("ok:backup-m");
    expect(calls).toEqual(["primary-m", "backup-m"]);
    expect(events.map((e) => e.event)).toEqual(["fail", "success"]);
    expect(events[1].attemptIndex).toBe(1);
  });

  it("spends retryCount rounds per model before switching", async () => {
    const calls: string[] = [];
    const result = await runWithModelChain(async (model) => {
      calls.push(model);
      if (calls.length < 3) throw RETRYABLE();
      return `ok:${model}`;
    }, {
      primaryModel: "primary-m",
      label: "writer",
      chain: chainOf([{ model: "primary-m" }, { model: "backup-m" }], 2),
    });
    expect(result).toBe("ok:backup-m");
    expect(calls).toEqual(["primary-m", "primary-m", "backup-m"]);
  });

  it("rethrows non-retryable errors without switching models", async () => {
    const calls: string[] = [];
    await expect(
      runWithModelChain(async (model) => {
        calls.push(model);
        throw FATAL();
      }, {
        primaryModel: "primary-m",
        label: "writer",
        chain: chainOf([{ model: "primary-m" }, { model: "backup-m" }]),
      }),
    ).rejects.toThrow("401");
    expect(calls).toEqual(["primary-m"]);
  });

  it("rethrows the last error after all attempts exhaust", async () => {
    const calls: string[] = [];
    await expect(
      runWithModelChain(async (model) => {
        calls.push(model);
        throw RETRYABLE();
      }, {
        primaryModel: "primary-m",
        label: "writer",
        chain: chainOf([{ model: "primary-m" }, { model: "backup-m" }]),
      }),
    ).rejects.toThrow("429");
    expect(calls).toEqual(["primary-m", "backup-m"]);
  });

  it("rethrows immediately when the signal is aborted", async () => {
    const controller = new AbortController();
    const calls: string[] = [];
    await expect(
      runWithModelChain(async (model) => {
        calls.push(model);
        controller.abort();
        throw RETRYABLE();
      }, {
        primaryModel: "primary-m",
        label: "writer",
        signal: controller.signal,
        chain: chainOf([{ model: "primary-m" }, { model: "backup-m" }]),
      }),
    ).rejects.toThrow();
    expect(calls).toEqual(["primary-m"]);
  });
});
