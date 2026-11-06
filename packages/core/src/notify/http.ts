/**
 * Notify channels share one bounded fetch: a webhook endpoint that never
 * responds must not hang the pipeline run that triggered the notification.
 * 899: all four senders previously issued bare fetches with no signal, so a
 * stalled endpoint hung the caller indefinitely (same family as the buffered
 * LLM request hole fixed in 898, different budget — notifications are small
 * POSTs, not generations).
 */

const DEFAULT_NOTIFY_TIMEOUT_MS = 15_000;
const ENV_NOTIFY_TIMEOUT = "INKOS_NOTIFY_TIMEOUT_MS";

/** Positive-integer parse with fallback (provider readPositiveTimeout semantics). */
function resolveNotifyTimeoutMs(env: NodeJS.ProcessEnv = process.env): number {
  const raw = Number(env[ENV_NOTIFY_TIMEOUT]);
  return Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : DEFAULT_NOTIFY_TIMEOUT_MS;
}

export async function notifyFetch(url: string, init: RequestInit = {}, env: NodeJS.ProcessEnv = process.env): Promise<Response> {
  return fetch(url, { ...init, signal: AbortSignal.timeout(resolveNotifyTimeoutMs(env)) });
}
