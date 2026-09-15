import { useState } from "react";
import { Loader2 } from "lucide-react";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";

interface DeconChapterInput {
  readonly chapter: number;
  readonly characters: string[];
  readonly events: string;
  readonly chapterType?: string;
  readonly hookActivity?: string;
}

interface DeconstructResponse {
  readonly markdown: string;
  readonly path: string;
  readonly publishedMaterialId?: string;
}

/**
 * G5/351 号：拆书工作台（书籍详情挂载，轻量版）。
 * 粘贴/编辑逐章拆书证据（章号｜人物｜事件｜章型｜伏笔动静）→
 * 统一拆书面端点聚合 → 四档档案 + 节奏卖点 + 可发布 markdown；
 * 可选一键发布到项目材料池（供参考资料绑定与 G1 召回）。
 */
export function DeconstructPanel({ bookId }: { bookId: string }) {
  const [text, setText] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [markdown, setMarkdown] = useState<string | null>(null);

  const parseChapters = (): DeconChapterInput[] => {
    const chapters: DeconChapterInput[] = [];
    for (const line of text.split("\n")) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      // 行格式：章号 | 人物(顿号/逗号分隔) | 事件 | 章型(可选) | 伏笔动静(可选)
      const parts = trimmed.split("|").map((part) => part.trim());
      const chapter = Number.parseInt(parts[0] ?? "", 10);
      if (!Number.isFinite(chapter)) continue;
      chapters.push({
        chapter,
        characters: (parts[1] ?? "").split(/[,，、]/).map((x) => x.trim()).filter(Boolean),
        events: parts[2] ?? "",
        ...(parts[3] ? { chapterType: parts[3] } : {}),
        ...(parts[4] ? { hookActivity: parts[4] } : {}),
      });
    }
    return chapters;
  };

  const run = async () => {
    const chapters = parseChapters();
    if (chapters.length === 0) {
      setNotice(tr("没有可解析的章节行（格式：章号|人物|事件|章型|伏笔）", "No parseable lines (chapter|characters|events|type|hooks)"));
      return;
    }
    setBusy(true);
    setNotice("");
    try {
      const data = await fetchJson<DeconstructResponse>(
        `/books/${encodeURIComponent(bookId)}/deconstruct`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ chapters, depth: "full", language: tr("zh", "en") === "zh" ? "zh" : "en" }),
        },
      );
      setMarkdown(data.markdown);
      setNotice(tr("已生成并落盘。", "Generated and saved."));
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };

  const publish = async () => {
    const chapters = parseChapters();
    if (chapters.length === 0) {
      setNotice(tr("没有可解析的章节行", "No parseable lines"));
      return;
    }
    setBusy(true);
    try {
      const data = await fetchJson<DeconstructResponse>(
        `/books/${encodeURIComponent(bookId)}/deconstruct`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            chapters,
            depth: "full",
            publish: true,
            ...(name ? { name } : {}),
            language: tr("zh", "en") === "zh" ? "zh" : "en",
          }),
        },
      );
      setMarkdown(data.markdown);
      setNotice(
        data.publishedMaterialId
          ? tr(`已发布到材料池：${data.publishedMaterialId}`, `Published to materials: ${data.publishedMaterialId}`)
          : tr("已生成。", "Generated."),
      );
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("拆书工作台", "Deconstruction workbench")}
      </h3>
      <textarea
        value={text}
        onChange={(event) => setText(event.target.value)}
        rows={4}
        placeholder={tr(
          "每行一章：章号 | 出场人物 | 事件梗概 | 章型 | 伏笔动静",
          "One chapter per line: no | characters | events | type | hooks",
        )}
        className="w-full text-xs border border-border rounded-md px-2 py-1.5 bg-background font-mono"
      />
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder={tr("产物名称（可选）", "export name (optional)")}
          className="text-xs border border-border rounded-md px-2 py-1.5 bg-background"
        />
        <button
          onClick={run}
          disabled={busy}
          className="px-3 py-1.5 text-xs rounded-md bg-primary text-primary-foreground disabled:opacity-30 flex items-center gap-1.5"
        >
          {busy ? <Loader2 size={12} className="animate-spin" /> : null}
          {tr("分析", "Analyze")}
        </button>
        <button
          onClick={publish}
          disabled={busy}
          className="px-3 py-1.5 text-xs rounded-md border border-border hover:bg-muted/30 disabled:opacity-30"
        >
          {tr("发布到材料池", "Publish to materials")}
        </button>
        {notice && <span className="text-xs text-muted-foreground">{notice}</span>}
      </div>
      {markdown && (
        <pre className="text-[11px] leading-relaxed whitespace-pre-wrap text-muted-foreground max-h-48 overflow-auto border border-border/40 rounded-md p-2">
          {markdown}
        </pre>
      )}
    </div>
  );
}
