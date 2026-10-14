// ── R36 会话树化（636-639 号；692 号随上游 2.0 合并重施）──
//
// 分支 = 追加 branch_moved 事件（append-only 不破坏）+ head 指针移动（replay
// 重建）。`toSeq` = 新 head 的事件 seq；null = resetLeaf（head 置空，后续写入
// 开新链根）。弃用路径零删改、可再 branch 回去。调用方负责忙判定（轮进行中
// 不 branch，保证单请求事件整体在同一链上）。会话不存在（无 transcript）返回
// null。原实现在 fork book-session-store.ts；2.0 重写该文件后本模块独立承载，
// 事件 schema（branch_moved/parentSeq）见 session-transcript-schema.ts（688 号
// 加法合并保留）。与 engine-rs session_branch 同水位对偶。

import {
  activeChainEvents,
  appendTranscriptEvents,
  readTranscriptEvents,
  transcriptHead,
} from "./session-transcript.js";

export interface BranchBookSessionResult {
  readonly head: number | null;
  readonly branchCount: number;
}

/**
 * R36 会话树化（636 号）：分支 = 追加一条 branch_moved 事件（append-only 不
 * 破坏）+ head 指针移动（replay 重建）。`toSeq` = 新 head 的事件 seq；null =
 * resetLeaf（head 置空，后续写入开新链根）。弃用路径零删改、可再 branch 回
 * 去。调用方负责忙判定（轮进行中不 branch，保证单请求事件整体在同一链上）。
 * 会话不存在（无 transcript）返回 null。
 */
export async function branchBookSession(
  projectRoot: string,
  sessionId: string,
  toSeq: number | null,
): Promise<BranchBookSessionResult | null> {
  let appended = 0;
  await appendTranscriptEvents(projectRoot, sessionId, ({ events, nextSeq }) => {
    if (events.length === 0) return [];
    const branchTargets = new Set(events.map((event) => event.seq));
    if (toSeq !== null && !branchTargets.has(toSeq)) {
      throw new BranchTargetNotFoundError(toSeq);
    }
    appended = 1;
    const fromSeq = transcriptHead(events);
    return [{
      type: "branch_moved",
      version: 1,
      sessionId,
      seq: nextSeq,
      timestamp: Date.now(),
      fromSeq,
      toSeq,
    }];
  });
  if (!appended) return null;

  const events = await readTranscriptEvents(projectRoot, sessionId);
  return {
    head: transcriptHead(events),
    branchCount: events.filter((event) => event.type === "branch_moved").length,
  };
}

/** branch 目标 seq 不在事件流中（服务端映射 400 INVALID_BRANCH_TARGET）。 */
export class BranchTargetNotFoundError extends Error {
  constructor(readonly toSeq: number) {
    super(`Branch target seq ${toSeq} not found in transcript`);
    this.name = "BranchTargetNotFoundError";
  }
}

/**
 * R36 分支点读面（638 号）：一个可回退分支点 = 一条已提交请求的
 * request_committed 事件（该轮事件 seq 最大的收尾点——消息先于 commit 落盘，
 * branch 到它 = 「保留到该轮结束」）。preview 取同 requestId 的
 * request_started.input（每轮必有、纯文本，免解析 message 载荷形状）；
 * onActiveChain 标记该点是否在当前 head 链上（false = 已弃用分支，可切回）。
 * 失败轮（request_failed 无 committed）不是分支点。会话不存在返回 null。
 */
export interface SessionBranchPoint {
  readonly seq: number;
  readonly requestId: string;
  readonly timestamp: number;
  readonly preview: string;
  readonly onActiveChain: boolean;
}

export interface SessionBranchPointsResult {
  readonly sessionId: string;
  readonly head: number | null;
  readonly branchCount: number;
  readonly points: ReadonlyArray<SessionBranchPoint>;
}

export async function deriveSessionBranchPoints(
  projectRoot: string,
  sessionId: string,
): Promise<SessionBranchPointsResult | null> {
  const events = await readTranscriptEvents(projectRoot, sessionId);
  if (events.length === 0) return null;
  const chainSeqs = new Set(activeChainEvents(events).map((event) => event.seq));
  const startedInputs = new Map<string, string>();
  for (const event of events) {
    if (event.type === "request_started") startedInputs.set(event.requestId, event.input);
  }
  const points: SessionBranchPoint[] = [];
  for (const event of events) {
    if (event.type !== "request_committed") continue;
    points.push({
      seq: event.seq,
      requestId: event.requestId,
      timestamp: event.timestamp,
      preview: startedInputs.get(event.requestId) ?? "",
      onActiveChain: chainSeqs.has(event.seq),
    });
  }
  return {
    sessionId,
    head: transcriptHead(events),
    branchCount: events.filter((event) => event.type === "branch_moved").length,
    points,
  };
}
