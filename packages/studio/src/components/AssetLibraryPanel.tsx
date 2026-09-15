import { useEffect, useState } from "react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface LibraryAsset {
  readonly id: string;
  readonly kind: "genre-base" | "progression-mode" | "world-sample";
  readonly name: string;
  readonly body: string;
  readonly expectations: ReadonlyArray<string>;
  readonly taboos: ReadonlyArray<string>;
  readonly samples: ReadonlyArray<string>;
}

interface Snapshot {
  readonly kind: string;
  readonly assets: ReadonlyArray<LibraryAsset>;
  readonly seeded: boolean;
}

const KINDS: ReadonlyArray<{ value: LibraryAsset["kind"]; label: string; en: string }> = [
  { value: "genre-base", label: "题材基底", en: "Genre base" },
  { value: "progression-mode", label: "推进模式", en: "Progression mode" },
  { value: "world-sample", label: "世界样本", en: "World sample" },
];

/**
 * R4/364 号：三库资产管理面板（书籍详情挂载）。
 * 列表 / 新增 / 删除 / 便携包导入导出；有 bookId 时支持「采用到本书」
 * （两段式：通用样本≠本书世界——采用写入本书材料池，进 G1 召回链）。
 */
export function AssetLibraryPanel({ bookId }: { bookId?: string }) {
  const [kind, setKind] = useState<LibraryAsset["kind"]>("genre-base");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [notice, setNotice] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [form, setForm] = useState<{ name: string; body: string; expectations: string; taboos: string }>({
    name: "", body: "", expectations: "", taboos: "",
  });

  const load = async (target: LibraryAsset["kind"] = kind) => {
    try {
      const data = await fetchJson<Snapshot>(`/asset-library/${target}`);
      setSnapshot(data);
    } catch {
      setSnapshot(null);
    }
  };

  useEffect(() => {
    void load(kind);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind]);

  const upsert = async () => {
    if (!form.name.trim() || !form.body.trim()) return;
    const splitLines = (value: string) =>
      value.split("\n").map((line) => line.trim()).filter(Boolean);
    try {
      await fetchJson(`/asset-library/${kind}/assets`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          asset: {
            id: form.name
              ? form.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 64) || `asset_${Date.now()}`
              : `asset_${Date.now()}`,
            kind,
            name: form.name.trim(),
            body: form.body.trim(),
            expectations: splitLines(form.expectations),
            taboos: splitLines(form.taboos),
            samples: [],
          },
        }),
      });
      setForm({ name: "", body: "", expectations: "", taboos: "" });
      setNotice(tr("已保存", "Saved"));
      await load(kind);
    } catch {
      setNotice(tr("保存失败（检查格式）", "Save failed (check format)"));
    }
  };

  const remove = async (id: string) => {
    try {
      await fetchJson(`/asset-library/${kind}/assets/${encodeURIComponent(id)}`, { method: "DELETE" });
      await load(kind);
    } catch {
      setNotice(tr("删除失败（内置种子不可删）", "Delete failed (builtin seeds are protected)"));
    }
  };

  const exportLibrary = async () => {
    try {
      const payload = await fetchJson<unknown>(`/asset-library/${kind}/export`);
      const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `asset-library-${kind}.json`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch {
      setNotice(tr("导出失败", "Export failed"));
    }
  };

  const importLibrary = async (file: File) => {
    try {
      const text = await file.text();
      const result = await fetchJson<{ added?: number; overwritten?: number; skipped?: number; errors?: string[] }>(
        `/asset-library/${kind}/import`,
        { method: "POST", headers: { "Content-Type": "application/json" }, body: text },
      );
      setNotice(tr(`导入完成：新增 ${result.added ?? 0} / 覆盖 ${result.overwritten ?? 0} / 跳过 ${result.skipped ?? 0}`, `Imported: +${result.added ?? 0} / ~${result.overwritten ?? 0} / =${result.skipped ?? 0}`));
      await load(kind);
    } catch {
      setNotice(tr("导入失败（检查包格式）", "Import failed (check package format)"));
    }
  };

  const adopt = async (id: string) => {
    if (!bookId) return;
    try {
      const result = await fetchJson<{ published: ReadonlyArray<unknown> }>(
        `/books/${encodeURIComponent(bookId)}/adopt-library-assets`,
        { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ refs: [{ kind, id }] }) },
      );
      setNotice(tr(`已采用 ${result.published.length} 项到本书参考资料`, `Adopted ${result.published.length} asset(s) into this book's references`));
    } catch {
      setNotice(tr("采用失败", "Adopt failed"));
    }
  };

  const inputClass = "w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background";

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
          {tr("三库资产", "Asset library")}
        </h3>
        <div className="flex items-center gap-1">
          {KINDS.map((entry) => (
            <button
              key={entry.value}
              onClick={() => {
                setKind(entry.value);
                // id 由名称派生不带库前缀，跨库同 id 资产会沿用旧展开态串显——切库即清
                setExpandedId(null);
              }}
              className={`px-2 py-1 text-[11px] rounded-md border ${kind === entry.value ? "border-primary text-primary font-bold" : "border-border text-muted-foreground"}`}
            >
              {tr(entry.label, entry.en)}
            </button>
          ))}
        </div>
      </div>
      <div className="flex items-center gap-2">
        <button onClick={exportLibrary} className="px-2 py-1 text-[11px] rounded-md border border-border text-muted-foreground">
          {tr("导出包", "Export")}
        </button>
        <label className="px-2 py-1 text-[11px] rounded-md border border-border text-muted-foreground cursor-pointer">
          {tr("导入包", "Import")}
          <input
            type="file"
            accept="application/json"
            className="hidden"
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) void importLibrary(file);
              event.target.value = "";
            }}
          />
        </label>
        {snapshot?.seeded && (
          <span className="text-[11px] text-muted-foreground italic">
            {tr("当前为内置种子（未落盘）", "showing builtin seeds (not saved yet)")}
          </span>
        )}
        {notice && <span className="text-[11px] text-emerald-600">{notice}</span>}
      </div>
      <div className="space-y-1">
        {(snapshot?.assets ?? []).map((asset) => (
          <div key={asset.id} className="rounded-md border border-border/60 px-2 py-1.5 space-y-1">
            <div className="flex items-center gap-2">
              <button
                onClick={() => setExpandedId(expandedId === asset.id ? null : asset.id)}
                className="text-xs font-bold text-left flex-1"
              >
                {asset.name}
                <span className="font-mono text-[10px] text-muted-foreground ml-1">{asset.id}</span>
              </button>
              {bookId && (
                <button
                  onClick={() => void adopt(asset.id)}
                  className="px-2 py-0.5 text-[11px] rounded-md bg-primary text-primary-foreground"
                >
                  {tr("采用到本书", "Adopt")}
                </button>
              )}
              <button
                onClick={() => void remove(asset.id)}
                className="px-2 py-0.5 text-[11px] rounded-md border border-border text-muted-foreground"
              >
                {tr("删", "Del")}
              </button>
            </div>
            {expandedId === asset.id && (
              <div className="text-[11px] text-muted-foreground space-y-1">
                <p>{asset.body}</p>
                {asset.expectations.length > 0 && (
                  <p>{tr("期待", "Expectations")}: {asset.expectations.join("；")}</p>
                )}
                {asset.taboos.length > 0 && (
                  <p>{tr("禁忌", "Taboos")}: {asset.taboos.join("；")}</p>
                )}
                {asset.samples.length > 0 && (
                  <p>{tr("样本", "Samples")}: {asset.samples.length}</p>
                )}
              </div>
            )}
          </div>
        ))}
      </div>
      <div className="space-y-1 border-t border-border/60 pt-2">
        <input
          value={form.name}
          onChange={(event) => setForm((prev) => ({ ...prev, name: event.target.value }))}
          placeholder={tr("名称（新增/覆盖同 id）", "Name (new entry / overwrite by id)")}
          className={inputClass}
        />
        <textarea
          value={form.body}
          onChange={(event) => setForm((prev) => ({ ...prev, body: event.target.value }))}
          placeholder={tr("正文：核心引擎/范式说明", "Body: core engine / paradigm notes")}
          rows={2}
          className={inputClass}
        />
        <textarea
          value={form.expectations}
          onChange={(event) => setForm((prev) => ({ ...prev, expectations: event.target.value }))}
          placeholder={tr("读者期待（每行一条）", "Expectations (one per line)")}
          rows={2}
          className={inputClass}
        />
        <textarea
          value={form.taboos}
          onChange={(event) => setForm((prev) => ({ ...prev, taboos: event.target.value }))}
          placeholder={tr("禁忌（每行一条）", "Taboos (one per line)")}
          rows={2}
          className={inputClass}
        />
        <button
          onClick={upsert}
          disabled={!form.name.trim() || !form.body.trim()}
          className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30"
        >
          {tr("保存资产", "Save asset")}
        </button>
      </div>
    </div>
  );
}
