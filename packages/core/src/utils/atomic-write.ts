/**
 * R29 持久层原子写（v5 五轮规划 P2 末件，404 号）。
 *
 * `writeTextAtomic` / `writeJsonAtomic`：先写同目录唯一临时文件，再 rename
 * 就位——进程中途崩溃只留孤儿临时文件，目标文件要么旧内容、要么完整新内容。
 * 吸收 WNW v6.2.1 的 atomic_write_json 思路：rename 目标被占用（Windows
 * 文件锁：EPERM/EBUSY）时**线性退避重试**，而不是一次失败就丢内容。
 *
 * 双端：Rust 镜像 `engine-rs/src/utils/atomic_file_set.rs` 的
 * `write_file_atomic`（199 号既有原子替换 + 本次新增同款重试）。
 * 重试决策由共享 golden 锁定：`golden/atomic-write-vectors.json`
 * （TS `golden-atomic-write.test.ts` / RS `tests/golden_atomic_write_diff.rs`）。
 */

/** rename 失败后的最大重试次数（首次尝试 + 3 = 至多 4 次）。 */
export const ATOMIC_WRITE_MAX_RETRIES = 3;
/** 重试基础退避（线性：attempt × delayMs）。 */
export const ATOMIC_WRITE_RETRY_DELAY_MS = 50;
/** 可重试的错误码（Windows 文件锁；EPERM 也出现在 rename 覆盖只读/占用目标时）。 */
export const ATOMIC_WRITE_RETRYABLE_CODES = ["EPERM", "EBUSY"] as const;

/**
 * 重试决策（纯函数，golden 锁）：错误码在重试集合内且重试预算未用尽 → 重试。
 * `attempt` 从 0 计（0 = 首次失败后的决策）。
 */
export function atomicWriteRetryDecision(
  errorCode: string | null,
  attempt: number,
  maxRetries: number = ATOMIC_WRITE_MAX_RETRIES,
): boolean {
  if (errorCode === null) return false;
  if (!(ATOMIC_WRITE_RETRYABLE_CODES as ReadonlyArray<string>).includes(errorCode)) return false;
  return attempt >= 0 && attempt < maxRetries;
}

/** Node fs 错误 → 重试决策用的错误码（不可重试/无码 → null）。 */
export function atomicWriteErrorCode(error: unknown): string | null {
  const code = (error as { code?: unknown } | null)?.code;
  return typeof code === "string" ? code : null;
}

/**
 * 单文件原子替换：tmp（同目录隐藏名，含 pid+uuid）→ rename。
 * rename 遇文件锁（EPERM/EBUSY）线性退避重试；放弃时清理 tmp 再抛错。
 * options 供测试注入（retries/delayMs/sleep/rename）。
 */
export async function writeTextAtomic(
  path: string,
  content: string,
  options?: {
    readonly retries?: number;
    readonly delayMs?: number;
    readonly sleep?: (ms: number) => Promise<void>;
    readonly rename?: (from: string, to: string) => Promise<void>;
  },
): Promise<void> {
  const { dirname, basename, join: joinPath } = await import("node:path");
  const { randomUUID } = await import("node:crypto");
  const fs = await import("node:fs/promises");

  const retries = options?.retries ?? ATOMIC_WRITE_MAX_RETRIES;
  const delayMs = options?.delayMs ?? ATOMIC_WRITE_RETRY_DELAY_MS;
  const sleep = options?.sleep ?? ((ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)));
  const rename = options?.rename ?? ((from: string, to: string) => fs.rename(from, to));

  const dir = dirname(path);
  const name = basename(path);
  const tmp = joinPath(dir, `.${name}.tmp-${process.pid}-${randomUUID()}`);

  await fs.writeFile(tmp, content, "utf-8");
  let attempt = 0;
  for (;;) {
    try {
      await rename(tmp, path);
      return;
    } catch (error) {
      const code = atomicWriteErrorCode(error);
      if (!atomicWriteRetryDecision(code, attempt, retries)) {
        await fs.rm(tmp, { force: true }).catch(() => undefined);
        throw error;
      }
      attempt += 1;
      await sleep(delayMs * attempt);
    }
  }
}

/** JSON 便捷版（紧凑序列化；调用方如需缩进自行 stringify 后走 writeTextAtomic）。 */
export async function writeJsonAtomic(
  path: string,
  value: unknown,
  options?: {
    readonly retries?: number;
    readonly delayMs?: number;
    readonly sleep?: (ms: number) => Promise<void>;
  },
): Promise<void> {
  return writeTextAtomic(path, JSON.stringify(value), options);
}
