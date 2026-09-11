import { useEffect, useState } from "react";
import { AlertTriangle, Loader2 } from "lucide-react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface QualityDebt {
  readonly debtId: string;
  readonly bookId: string;
  readonly chapter: number;
  readonly issueCategory: string;
  readonly severity: string;
  readonly status: "open" | "deferred" | "resolved";
  readonly createdAt: string;
  readonly followUpNote?: string;
}

const STATUS_STYLE: Record<string, string> = {
  open: "bg-red-500/10 text-red-600",
  deferred: "bg-amber-500/10 text-amber-600",
  resolved: "bg-emerald-500/10 text-emerald-600",
};

function statusLabel(status: string): string {
  switch (status) {
    case "open": return tr("待处理", "Open");
    case "deferred": return tr("已降级挂账", "Deferred");
    case "resolved": return tr("已解决", "Resolved");
    default: return status;
  }
}

/**
 * G3/337 号：质量债务清单（书籍详情挂载）。
 * 只展示未决债务（open/deferred）；resolved 计数收进尾部摘要。
 * 无债务时整卡不渲染——零噪音。
 */
export function QualityDebtsCard({ bookId }: { bookId: string }) {
  const [debts, setDebts] = useState<ReadonlyArray<QualityDebt> | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetchJson<{ debts?: ReadonlyArray<QualityDebt> }>(
      `/books/${encodeURIComponent(bookId)}/quality-debts`,
    )
      .then((r) => {
        if (!cancelled) setDebts(r.debts ?? []);
      })
      .catch(() => {
        if (!cancelled) setDebts([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [bookId]);

  if (loading && debts === null) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Loader2 size={12} className="animate-spin" />
        {tr("加载质量债务…", "Loading quality debts…")}
      </div>
    );
  }

  const pending = (debts ?? []).filter((d) => d.status !== "resolved");
  const resolvedCount = (debts ?? []).length - pending.length;
  if (pending.length === 0) return null;

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground flex items-center gap-2">
        <AlertTriangle size={13} className="text-amber-500" aria-hidden />
        {tr("质量债务", "Quality debts")}
        <span className="font-normal normal-case">{tr("（未决", " (pending")} {pending.length}</span>
        {resolvedCount > 0 && (
          <span className="font-normal normal-case">
            {tr("· 已解决", "· resolved")} {resolvedCount}
          </span>
        )}
        <span className="font-normal normal-case">)</span>
      </h3>
      <ul className="space-y-1.5">
        {pending.map((debt) => (
          <li key={debt.debtId} className="flex flex-wrap items-center gap-2 text-xs">
            <span className={`px-2 py-0.5 rounded-full font-bold ${STATUS_STYLE[debt.status] ?? ""}`}>
              {statusLabel(debt.status)}
            </span>
            <span className="text-foreground">
              {tr("第", "Ch.")} {debt.chapter} {tr("章", "")}
            </span>
            <span className="text-muted-foreground">{debt.issueCategory}</span>
            {debt.followUpNote && (
              <span className="text-muted-foreground/70 line-clamp-1">{debt.followUpNote}</span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
