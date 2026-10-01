import { useEffect, useState } from "react";
import { Gauge } from "lucide-react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface TokenMeterSnapshotView {
  readonly model: string | null;
  readonly heuristicTokens: number;
  readonly anchoredTokens: number;
  readonly tokens: number;
  readonly anchorValid: boolean;
  readonly source: "usage" | "estimate";
  readonly coverage: number;
  readonly inputWindow: number;
  readonly overWindow: boolean;
  readonly surfaceNodes: number;
}

/** 5s 轮询：聊天轮异步完成，快照随轮末落账（Rust/Node 同点留痕）。 */
const POLL_MS = 5_000;

function formatTokens(value: number): string {
  if (value >= 10_000) return `${(value / 1000).toFixed(1)}k`;
  return String(value);
}

/**
 * R41/564 号：会话上下文计量徽标（聊天输入状态条挂载，与 Rust/Node
 * GET /api/v1/context-meter 同构）。只读观测：末轮请求面 token（usage 锚点
 * 校准/启发式双口径）+ 超窗告警色。无快照（未跑过聊天轮/重启后）不渲染。
 */
export function ContextMeterBadge({ sessionId }: { sessionId: string | null }) {
  const [snapshot, setSnapshot] = useState<TokenMeterSnapshotView | null>(null);

  useEffect(() => {
    if (!sessionId) {
      setSnapshot(null);
      return;
    }
    let cancelled = false;
    const load = () => {
      fetchJson<TokenMeterSnapshotView>(
        `/context-meter?sessionId=${encodeURIComponent(sessionId)}`,
      )
        .then((data) => {
          if (!cancelled) setSnapshot(data);
        })
        .catch(() => {
          // 404（该会话尚无快照）与其他错误同样静默——观测面不打扰。
          if (!cancelled) setSnapshot(null);
        });
    };
    load();
    const timer = window.setInterval(load, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [sessionId]);

  if (!sessionId || !snapshot) return null;

  const title = [
    `${tr("上下文计量", "Context meter")}: ${snapshot.tokens} tok (${snapshot.source})`,
    `${tr("启发式", "heuristic")} ${snapshot.heuristicTokens}`,
    snapshot.anchorValid
      ? `${tr("锚点校准", "anchored")} ${snapshot.anchoredTokens} · ${tr("覆盖", "coverage")} ${(snapshot.coverage * 100).toFixed(1)}%`
      : tr("无 usage 锚点", "no usage anchor"),
    snapshot.model ? `${tr("模型", "model")} ${snapshot.model}` : null,
    snapshot.inputWindow > 0
      ? `${tr("窗口", "window")} ${snapshot.inputWindow}`
      : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <span
      data-slot="context-meter-badge"
      title={title}
      className={`ml-auto flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium transition-colors ${
        snapshot.overWindow
          ? "bg-red-500/10 text-red-600"
          : "text-muted-foreground/70"
      }`}
    >
      <Gauge size={13} />
      <span>{formatTokens(snapshot.tokens)}</span>
      <span className="hidden md:inline">tok</span>
      {snapshot.overWindow && <span>{tr("超窗", "over")}</span>}
    </span>
  );
}
