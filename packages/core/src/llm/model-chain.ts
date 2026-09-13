import { isRetryableLLMError } from "./provider.js";
import type { ResolvedTaskModelChain } from "../models/task-routing.js";
import { globalRunLog } from "../utils/run-log.js";

/**
 * R25/399 号：模型尝试链执行器——`resolveTaskModelChain` 的调用缝侧。
 *
 * 包在既有同模型瞬态重试（provider 层 `withTransientLLMRetry` / Rust
 * `AgentRouter::chat` 内环）**之外**：每个链目 model 跑 retryCount 轮，轮内
 * 失败按 `isRetryableLLMError` 分类——瞬态（429/5xx/网络/流中断）继续下一轮/
 * 下一模型；非瞬态（中止/上下文超限/鉴权/4xx/模型不存在）立即抛出不切换。
 * 零配置（无 chain 或 [primary]+1）→ 单次直通，行为与现版本逐字节一致。
 *
 * 双端：Rust 镜像 `engine-rs/src/llm/agent_router.rs` `chat`（flaky mock
 * 行为测锁定接管/不切换两语义）。
 */

export interface ModelChainEvent {
  readonly event: "fail" | "success";
  readonly model: string;
  /** 链内序号（0 = primary）。 */
  readonly attemptIndex: number;
  /** 轮次（1 起）。 */
  readonly round: number;
  readonly error?: unknown;
}

export interface ModelChainRunOptions {
  /** 未配置接管链时传 undefined——直通 primaryModel。 */
  readonly chain?: ResolvedTaskModelChain;
  readonly primaryModel: string;
  /** 日志/遥测标签（agent 名）。 */
  readonly label?: string;
  readonly signal?: AbortSignal;
  /** 接管事件观察点（logger 留痕 / R26 运行遥测挂点）。 */
  readonly onEvent?: (event: ModelChainEvent) => void;
}

/**
 * R26/403 号：链内一轮的执行 + 运行遥测记录（只记元数据，不含 prompt/正文/
 * 错误原文）。零配置直通分支同样记录——tookOver 与 R25 接管成功日志同口径
 * （attemptIndex > 0 || round > 1）。
 */
async function runAndRecord<T>(
  run: (model: string) => Promise<T>,
  model: string,
  attemptIndex: number,
  round: number,
  label: string,
): Promise<T> {
  const startedAt = Date.now();
  try {
    const result = await run(model);
    globalRunLog.append({
      ts: new Date().toISOString(),
      agent: label,
      model,
      durationMs: Date.now() - startedAt,
      ok: true,
      attemptIndex,
      round,
      tookOver: attemptIndex > 0 || round > 1,
      errorKind: null,
    });
    return result;
  } catch (error) {
    globalRunLog.append({
      ts: new Date().toISOString(),
      agent: label,
      model,
      durationMs: Date.now() - startedAt,
      ok: false,
      attemptIndex,
      round,
      tookOver: attemptIndex > 0 || round > 1,
      errorKind: isRetryableLLMError(error) ? "transient" : "fatal",
    });
    throw error;
  }
}

export async function runWithModelChain<T>(
  run: (model: string) => Promise<T>,
  options: ModelChainRunOptions,
): Promise<T> {
  const chain = options.chain;
  const label = options.label ?? "unknown";
  if (!chain || (chain.attempts.length <= 1 && chain.retryCount <= 1)) {
    return runAndRecord(run, options.primaryModel, 0, 1, label);
  }
  let lastError: unknown;
  for (let attemptIndex = 0; attemptIndex < chain.attempts.length; attemptIndex++) {
    const model = attemptIndex === 0 ? options.primaryModel : chain.attempts[attemptIndex].model;
    for (let round = 1; round <= chain.retryCount; round++) {
      options.signal?.throwIfAborted();
      try {
        const result = await runAndRecord(run, model, attemptIndex, round, label);
        if (attemptIndex > 0 || round > 1) {
          options.onEvent?.({ event: "success", model, attemptIndex, round });
        }
        return result;
      } catch (error) {
        lastError = error;
        if (options.signal?.aborted) throw error;
        if (!isRetryableLLMError(error)) throw error;
        options.onEvent?.({ event: "fail", model, attemptIndex, round, error });
      }
    }
  }
  throw lastError;
}
