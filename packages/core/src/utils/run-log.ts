/**
 * R26 调用级运行遥测（v5 五轮规划 P0，403 号）。
 *
 * 每次 LLM 调用（链内一轮 = 一条）追加元数据记录到进程级环形缓冲：
 * agent / model / 耗时 ms / 成败 / 链内序号 / 轮次 / 是否接管 / 错误类。
 * **只记元数据，绝不记 prompt/正文/错误原文**（v5 隐私红线；错误原文
 * 走 inkos.log 的 R25 既有通道）。
 *
 * 双端：`engine-rs/src/utils/run_log.rs` 1:1；共享 golden
 * `golden/run-log-vectors.json`（TS `golden-run-log.test.ts` /
 * RS `tests/golden_run_log_diff.rs`）。
 */

/** 默认环形缓冲容量（v5 规格：200 条）。 */
export const DEFAULT_RUN_LOG_CAPACITY = 200;

export type RunLogErrorKind = "transient" | "fatal";

export interface RunLogEntry {
  /** ISO 8601 UTC 时间戳（记录时刻）。 */
  readonly ts: string;
  /** agent 名 / 调用标签。 */
  readonly agent: string;
  readonly model: string;
  /** 调用耗时（毫秒，整数）。 */
  readonly durationMs: number;
  readonly ok: boolean;
  /** 链内序号（0 = primary）。 */
  readonly attemptIndex: number;
  /** 轮次（1 起）。 */
  readonly round: number;
  /** R25 联动：非 primary 首轮命中即接管切换。 */
  readonly tookOver: boolean;
  readonly errorKind: RunLogErrorKind | null;
}

export interface RunLogSnapshot {
  /** 缓冲内现存记录（旧 → 新）。 */
  readonly entries: ReadonlyArray<RunLogEntry>;
  /** 累计追加数（含被逐出的旧记录）。 */
  readonly totalAppended: number;
}

export interface RunLogProjection {
  /** 累计调用数（含被逐出）。 */
  readonly total: number;
  /** 缓冲内现存条数。 */
  readonly kept: number;
  /** 现存记录中接管成功的次数（R25 联动展示）。 */
  readonly tookOverCount: number;
  /** 现存记录中失败次数。 */
  readonly failureCount: number;
  /** 最新在后（时间升序，环形缓冲自然序）。 */
  readonly entries: ReadonlyArray<RunLogEntry>;
}

export class RunLogBuffer {
  private items: RunLogEntry[] = [];
  private totalAppended = 0;

  constructor(readonly capacity: number = DEFAULT_RUN_LOG_CAPACITY) {}

  append(entry: RunLogEntry): void {
    this.totalAppended += 1;
    this.items.push(entry);
    if (this.items.length > this.capacity) {
      this.items.splice(0, this.items.length - this.capacity);
    }
  }

  snapshot(): RunLogSnapshot {
    return { entries: [...this.items], totalAppended: this.totalAppended };
  }
}

/**
 * 只读投影：聚合计数 + 可选截尾（limit = 返回最近 limit 条，仍按时间升序）。
 * limit 非正或缺省 → 全量。
 */
export function projectRunLog(
  snapshot: RunLogSnapshot,
  limit?: number,
): RunLogProjection {
  const entries = limit !== undefined && limit > 0
    ? snapshot.entries.slice(Math.max(0, snapshot.entries.length - limit))
    : snapshot.entries;
  return {
    total: snapshot.totalAppended,
    kept: snapshot.entries.length,
    tookOverCount: snapshot.entries.filter((entry) => entry.tookOver).length,
    failureCount: snapshot.entries.filter((entry) => !entry.ok).length,
    entries,
  };
}

/** 进程级单例（studio server / engine 各自进程内）。 */
export const globalRunLog = new RunLogBuffer();

/** 调用点记录入参——纯元数据（不含 prompt/正文/错误原文，v5 隐私红线）。 */
export interface RunLogRecordInput {
  /** agent 名 / 调用标签。 */
  readonly agent: string;
  readonly model: string;
  /** 调用起点（`Date.now()`），耗时由此推得。 */
  readonly startedAt: number;
  readonly ok: boolean;
  /** 链内序号（缺省 0 = primary）。 */
  readonly attemptIndex?: number;
  /** 轮次（缺省 1）。 */
  readonly round?: number;
  /** 失败类（成功时忽略，落 null）。 */
  readonly errorKind?: RunLogErrorKind | null;
}

/**
 * 1663 号：调用点记录器——403 号 `runAndRecord` 的 2.0 复座。
 *
 * 688 号上游合并退役 `llm/model-chain.ts` 时带走了唯一写入点，701 号回收的
 * GET /run-log 此后恒空（Node 回退腿 Dashboard「运行遥测」永不出现）——
 * 端点在位掩盖了写入面失踪。写入点现收敛到 2.0 两条 LLM 传输边界：
 * agent 路径 `agent/pi-stream.ts` observeModelStream、非 agent 路径
 * `llm/provider.ts` chatCompletion。
 *
 * tookOver 与 Rust 同口径：仅成功且 attemptIndex > 0 || round > 1
 * （非 primary 命中 = R25 接管成功；2.0 无模型链，attemptIndex 恒 0）。
 *
 * 1666 号（勘误 1663 口径备案）：`round` 是 **R25 链轮**，不是同模型瞬态
 * 重试的序号——Rust 的记录点在 `chat` 的 (chain_idx, round) 层，
 * `chat_transient` 内环的 429/5xx 退避重试不单列（活体锁
 * `engine-rs/tests/run_log_call_site.rs`）。调用点须把瞬态重试**折叠**进一条
 * （2.0 无链 → round 恒 1、tookOver 恒 false）；把重试序号喂进 round 会把
 * 同模型重试记成「接管」。
 */
export function recordRunLogCall(input: RunLogRecordInput): void {
  const attemptIndex = input.attemptIndex ?? 0;
  const round = input.round ?? 1;
  globalRunLog.append({
    ts: new Date().toISOString(),
    agent: input.agent,
    model: input.model,
    durationMs: Math.max(0, Date.now() - input.startedAt),
    ok: input.ok,
    attemptIndex,
    round,
    tookOver: input.ok && (attemptIndex > 0 || round > 1),
    errorKind: input.ok ? null : input.errorKind ?? "fatal",
  });
}
