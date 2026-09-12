import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface RoutingOverride {
  readonly model?: string;
  readonly service?: string;
  readonly temperature?: number;
  readonly maxTokens?: number;
}

interface Routing {
  readonly defaults?: RoutingOverride;
  readonly tasks?: Partial<Record<string, RoutingOverride>>;
}

const TASKS: ReadonlyArray<{ readonly kind: string; readonly zh: string; readonly en: string }> = [
  { kind: "writing", zh: "写作", en: "Writing" },
  { kind: "review", zh: "审改", en: "Review" },
  { kind: "repair", zh: "修复", en: "Repair" },
  { kind: "detect", zh: "检测", en: "Detection" },
  { kind: "analysis", zh: "分析", en: "Analysis" },
];

/**
 * G16/346 号：按任务模型路由——设置面集中管理。
 * 五类任务各覆盖 model（留空 = 用全局模型）；保存至项目级
 * .inkos/task-routing.json，写作/审计/修复/分析管线按任务解析生效。
 */
export function TaskRoutingPanel() {
  const [routing, setRouting] = useState<Routing>({});
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState("");

  useEffect(() => {
    let cancelled = false;
    fetchJson<{ routing: Routing | null }>("/task-routing")
      .then((r) => {
        if (!cancelled) setRouting(r.routing ?? {});
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const modelFor = (kind: string): string =>
    routing.tasks?.[kind]?.model ?? routing.defaults?.model ?? "";

  const setTaskModel = (kind: string, model: string) => {
    setRouting((previous) => {
      const tasks = { ...(previous.tasks ?? {}) };
      tasks[kind] = { ...tasks[kind], ...(model ? { model } : { model: undefined }) };
      return { ...previous, tasks };
    });
  };

  const save = async () => {
    setSaving(true);
    setNotice("");
    try {
      // 清理空 model 字段再保存。
      const tasks: Record<string, RoutingOverride> = {};
      for (const [kind, override] of Object.entries(routing.tasks ?? {})) {
        const cleaned: Record<string, unknown> = { ...override };
        if (!cleaned.model) delete cleaned.model;
        if (Object.keys(cleaned).length > 0) tasks[kind] = cleaned as RoutingOverride;
      }
      await fetchJson("/task-routing", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          routing: { ...(routing.defaults ? { defaults: routing.defaults } : {}), tasks },
        }),
      });
      setNotice(tr("已保存。新任务即时生效。", "Saved. Applies to new tasks immediately."));
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    setSaving(false);
  };

  if (!loaded) return null;

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-3">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("按任务模型路由", "Task model routing")}
      </h3>
      <p className="text-xs text-muted-foreground">
        {tr(
          "为不同任务绑定不同模型；留空则使用全局模型。",
          "Bind a model per task; leave blank to use the global model.",
        )}
      </p>
      <div className="space-y-2">
        {TASKS.map(({ kind, zh, en }) => (
          <div key={kind} className="flex items-center gap-3">
            <span className="w-16 text-xs text-foreground">{tr(zh, en)}</span>
            <input
              value={routing.tasks?.[kind]?.model ?? ""}
              onChange={(event) => setTaskModel(kind, event.target.value.trim())
              }
              placeholder={tr("使用全局模型", "use global model")}
              className="flex-1 text-xs border border-border rounded-md px-2 py-1.5 bg-background"
            />
          </div>
        ))}
      </div>
      <div className="flex items-center gap-3">
        <button
          onClick={save}
          disabled={saving}
          className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
        >
          {saving ? tr("保存中…", "Saving…") : tr("保存路由", "Save routing")}
        </button>
        {notice && <span className="text-xs text-muted-foreground">{notice}</span>}
      </div>
    </div>
  );
}
