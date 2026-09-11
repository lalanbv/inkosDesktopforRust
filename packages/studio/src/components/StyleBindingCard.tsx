import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface StyleProfileSummary {
  readonly name: string;
}

interface StyleBinding {
  readonly profileName: string;
  readonly enabledIds?: string[];
  readonly disabledIds?: string[];
  readonly maxGuidanceChars?: number;
}

/**
 * G4/340 号：写法绑定管理（书籍详情挂载，轻量版）。
 * 选择档案池中的档案并绑到本书；写作链会把生效特征作为
 * style-asset 条目注入 Selected Context（垫底参考层）。
 */
export function StyleBindingCard({ bookId }: { bookId: string }) {
  const [profiles, setProfiles] = useState<ReadonlyArray<StyleProfileSummary>>([]);
  const [binding, setBinding] = useState<StyleBinding | null>(null);
  const [selected, setSelected] = useState("");
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState("");

  useEffect(() => {
    let cancelled = false;
    void fetchJson<{ profiles?: ReadonlyArray<StyleProfileSummary> }>("/style-profiles")
      .then((r) => {
        if (cancelled) return;
        setProfiles(r.profiles ?? []);
      })
      .catch(() => undefined);
    void fetchJson<{ binding: StyleBinding | null }>(
      `/books/${encodeURIComponent(bookId)}/style-binding`,
    )
      .then((r) => {
        if (cancelled) return;
        setBinding(r.binding);
        if (r.binding) setSelected(r.binding.profileName);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [bookId]);

  const save = async () => {
    if (!selected) return;
    setSaving(true);
    setNotice("");
    try {
      await fetchJson(`/books/${encodeURIComponent(bookId)}/style-binding`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ profileName: selected }),
      });
      setBinding({ profileName: selected });
      setNotice(tr("已绑定。写作链将注入该档案的写法特征。", "Bound. The writing chain now injects these style features."));
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    setSaving(false);
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("写法绑定", "Style binding")}
      </h3>
      {profiles.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          {tr(
            "档案池为空。先用「文风」页分析参考文本并保存档案。",
            "Profile pool is empty. Analyze reference text on the Style page first.",
          )}
        </p>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <select
            value={selected}
            onChange={(event) => setSelected(event.target.value)}
            className="text-xs border border-border rounded-md px-2 py-1.5 bg-background"
          >
            <option value="">{tr("选择档案…", "Select a profile…")}</option>
            {profiles.map((profile) => (
              <option key={profile.name} value={profile.name}>
                {profile.name}
              </option>
            ))}
          </select>
          <button
            onClick={save}
            disabled={saving || !selected}
            className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
          >
            {saving ? tr("保存中…", "Saving…") : tr("绑定到本书", "Bind to book")}
          </button>
          {binding && (
            <span className="text-xs text-muted-foreground">
              {tr("当前绑定", "Bound to")}:
              {" "}
              <span className="text-foreground font-medium">{binding.profileName}</span>
            </span>
          )}
          {notice && <span className="text-xs text-muted-foreground">{notice}</span>}
        </div>
      )}
    </div>
  );
}
