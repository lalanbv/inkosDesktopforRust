import { useApi } from "../hooks/use-api";
import { nextActionFor, type AuthorErrorSeverity } from "@actalk/inkos-core/utils/author-error-catalog";
import type { Theme } from "../hooks/use-theme";
import type { TFunction } from "../hooks/use-i18n";
import { useColors } from "../hooks/use-colors";
import { Stethoscope, CheckCircle2, XCircle, AlertTriangle, Loader2 } from "lucide-react";

interface DoctorBookIssue {
  readonly bookId: string;
  readonly title: string;
  readonly kind: string;
  readonly chapter?: number;
}

interface DoctorChecks {
  readonly inkosJson: boolean;
  readonly projectEnv: boolean;
  readonly globalEnv: boolean;
  readonly booksDir: boolean;
  readonly llmConnected: boolean;
  readonly bookCount: number;
  /** 195 号：书籍级写作阻塞预警（旧后端缺省省略——渲染层判空跳过）。 */
  readonly bookIssues?: ReadonlyArray<DoctorBookIssue>;
}

interface Nav { toDashboard: () => void }

function CheckRow({ label, ok, detail }: { label: string; ok: boolean; detail?: string }) {
  return (
    <div className="flex items-center gap-3 py-3 border-b border-border/30 last:border-0">
      {ok ? (
        <CheckCircle2 size={18} className="text-emerald-500 shrink-0" />
      ) : (
        <XCircle size={18} className="text-destructive shrink-0" />
      )}
      <span className="text-sm font-medium flex-1">{label}</span>
      {detail && <span className="text-xs text-muted-foreground">{detail}</span>}
    </div>
  );
}

/** 195 号：书籍级 issue → 人类可读文案（双语走 i18n）。 */
function issueText(issue: DoctorBookIssue, t: TFunction): string {
  if (issue.kind === "state-degraded") {
    return t("doctor.issueStateDegraded").replace("{chapter}", String(issue.chapter ?? "?"));
  }
  return issue.kind;
}

/** R7/368 号：书籍级 issue → 错误目录下一步建议（与报告/通知同源）。 */
function issueNextAction(issue: DoctorBookIssue): string {
  // R7/368 号：state-degraded 为数据态问题（must-handle），其余预警按需确认。
  const severity: AuthorErrorSeverity = issue.kind === "state-degraded" ? "must-handle" : "needs-review";
  return nextActionFor(severity);
}

export function DoctorView({ nav, theme, t }: { nav: Nav; theme: Theme; t: TFunction }) {
  const c = useColors(theme);
  const { data, refetch } = useApi<DoctorChecks>("/doctor");
  const bookIssues = data?.bookIssues ?? [];

  return (
    <div className="space-y-8">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <button onClick={nav.toDashboard} className={c.link}>{t("bread.home")}</button>
        <span className="text-border">/</span>
        <span>{t("nav.doctor")}</span>
      </div>

      <div className="flex items-center justify-between">
        <h1 className="font-serif text-3xl flex items-center gap-3">
          <Stethoscope size={28} className="text-primary" />
          {t("doctor.title")}
        </h1>
        <button onClick={() => refetch()} className={`px-4 py-2 text-sm rounded-lg ${c.btnSecondary}`}>
          {t("doctor.recheck")}
        </button>
      </div>

      {!data ? (
        <div className="flex items-center justify-center py-12">
          <Loader2 size={24} className="animate-spin text-primary" />
        </div>
      ) : (
        <div className={`border ${c.cardStatic} rounded-lg p-5`}>
          <CheckRow label={t("doctor.inkosJson")} ok={data.inkosJson} />
          <CheckRow label={t("doctor.projectEnv")} ok={data.projectEnv} />
          <CheckRow label={t("doctor.globalEnv")} ok={data.globalEnv} />
          <CheckRow label={t("doctor.booksDir")} ok={data.booksDir} detail={`${data.bookCount} book(s)`} />
          <CheckRow label={t("doctor.llmApi")} ok={data.llmConnected} detail={data.llmConnected ? t("doctor.connected") : t("doctor.failed")} />
          {/* 195 号：书籍健康区——有阻塞问题时显示每条预警与修复指引。 */}
          <div className="pt-3" data-slot="doctor-book-health">
            {bookIssues.length === 0 ? (
              <div className="flex items-center gap-3 py-1">
                <CheckCircle2 size={18} className="text-emerald-500 shrink-0" />
                <span className="text-sm font-medium flex-1">{t("doctor.bookHealth")}</span>
                <span className="text-xs text-emerald-600 dark:text-emerald-400">{t("doctor.bookHealthOk")}</span>
              </div>
            ) : (
              bookIssues.map((issue) => (
                <div
                  key={`${issue.bookId}:${issue.kind}:${issue.chapter ?? ""}`}
                  className="flex items-start gap-3 py-2"
                  data-slot="doctor-book-issue"
                >
                  <AlertTriangle size={18} className="text-amber-500 shrink-0 mt-0.5" />
                  <div className="flex-1 min-w-0">
                    <div className="text-sm font-medium">{issue.title}</div>
                    <div className="text-xs text-amber-600 dark:text-amber-400 mt-0.5">{issueText(issue, t)}</div>
                    <div className="text-[11px] text-muted-foreground mt-0.5">{issueNextAction(issue)}</div>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      )}

      {data && (
        <div className={`px-4 py-3 rounded-lg text-sm font-medium ${
          data.inkosJson && (data.projectEnv || data.globalEnv) && data.llmConnected
            ? "bg-emerald-500/10 text-emerald-600"
            : "bg-amber-500/10 text-amber-600"
        }`}>
          {data.inkosJson && (data.projectEnv || data.globalEnv) && data.llmConnected
            ? t("doctor.allPassed")
            : t("doctor.someFailed")
          }
        </div>
      )}
    </div>
  );
}
