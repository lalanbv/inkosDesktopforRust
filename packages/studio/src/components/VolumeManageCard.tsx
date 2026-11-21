import { useApi, fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";
// 子路径导入（1640 号同族教训）：主入口 dist 拖 node:async_hooks 链进
// vite 前端 bundle——卷面纯函数走无 node 依赖的子路径。
import { projectVolumeGroups } from "@actalk/inkos-core/state/volumes";
import type { ChapterMeta } from "@actalk/inkos-core";
import { postApi } from "../hooks/use-api";

interface BookVolume {
  readonly id: string;
  readonly title: string;
  readonly order: number;
}

type ChapterRow = Pick<ChapterMeta, "number" | "volumeId">;

/**
 * R49/1644 号：卷/幕元数据管理卡（书籍详情挂载）。
 * 卷有序列表 CRUD（新建/改名/删卷合一走 PUT volumes 全量事务，删卷自动
 * 迁移章归「正文」默认卷）+ 分组投影（projectVolumeGroups）+ 章归属行级
 * 更新（PUT chapters/:num/volume）。卷是组织面非上下文层——不进装配注入。
 */
export function VolumeManageCard({ bookId, chapters }: { bookId: string; chapters: ReadonlyArray<ChapterRow> }) {
  const { data, refetch } = useApi<{ volumes: ReadonlyArray<BookVolume> }>(
    `/books/${encodeURIComponent(bookId)}/volumes`,
  );
  const volumes = data?.volumes ?? [];
  const groups = projectVolumeGroups(volumes, chapters);

  const saveVolumes = async (next: ReadonlyArray<BookVolume>) => {
    const body = await fetchJson<{ ok: boolean; volumes: ReadonlyArray<BookVolume> }>(
      `/books/${encodeURIComponent(bookId)}/volumes`,
      { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ volumes: next }) },
    );
    await refetch();
    return body;
  };

  const createVolume = async () => {
    const title = window.prompt(tr("新卷名称", "New volume title"));
    if (!title?.trim()) return;
    const id = `vol-${Date.now()}`;
    const next = [...volumes, { id, title: title.trim(), order: volumes.length + 1 }];
    try {
      await saveVolumes(next);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const renameVolume = async (id: string) => {
    const current = volumes.find((volume) => volume.id === id);
    const title = window.prompt(tr("改名", "Rename"), current?.title ?? "");
    if (!title?.trim() || title === current?.title) return;
    const next = volumes.map((volume) => (volume.id === id ? { ...volume, title: title.trim() } : volume));
    try {
      await saveVolumes(next);
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const deleteVolume = async (id: string) => {
    // 删卷迁移：归属章自动归「正文」默认卷（引擎事务面）。
    if (!window.confirm(tr("删除该卷？卷内章节将归入未分组。", "Delete this volume? Its chapters move to ungrouped."))) {
      return;
    }
    try {
      await saveVolumes(volumes.filter((volume) => volume.id !== id));
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  const assignChapter = async (number: number, volumeId: string) => {
    try {
      await postApi(`/books/${encodeURIComponent(bookId)}/chapters/${number}/volume`, {
        volumeId,
      });
      await refetch();
    } catch (e) {
      alert(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="rounded-lg border border-border/60 p-4 space-y-2">
      <h3 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
        {tr("卷/幕", "Volumes")}
      </h3>
      <p className="text-[10px] text-muted-foreground italic">
        {tr("组织层——不进写作上下文注入。", "Organizational layer — never injected into writing context.")}
      </p>
      <ul className="space-y-1">
        {volumes.map((volume) => (
          <li key={volume.id} className="flex items-center gap-2 text-sm">
            <span className="font-medium truncate">{volume.title}</span>
            <button
              onClick={() => void renameVolume(volume.id)}
              className="text-[10px] text-muted-foreground hover:text-foreground"
            >
              {tr("改名", "Rename")}
            </button>
            <button
              onClick={() => void deleteVolume(volume.id)}
              className="text-[10px] text-destructive hover:text-destructive/70"
            >
              {tr("删卷", "Delete")}
            </button>
          </li>
        ))}
        {volumes.length === 0 ? (
          <li className="text-xs text-muted-foreground italic">{tr("暂无卷——全部章节在未分组。", "No volumes — all chapters ungrouped.")}</li>
        ) : null}
      </ul>
      <button
        onClick={() => void createVolume()}
        className="w-full rounded-lg border border-dashed border-border/50 py-1.5 text-xs text-muted-foreground hover:text-foreground hover:border-primary/40 transition-colors"
      >
        {tr("＋ 新建卷", "＋ New volume")}
      </button>
      <div className="pt-2 space-y-1">
        <p className="text-[10px] uppercase tracking-widest text-muted-foreground/60">{tr("分组投影", "Grouping")}</p>
        {groups.map((group) => (
          <p key={group.volumeId ?? "default"} className="text-xs text-muted-foreground">
            <span className="font-medium">{group.title ?? tr("未分组（正文）", "Ungrouped")}</span>
            <span className="ml-1">
              {group.chapters.length > 0
                ? group.chapters.map((n) => `#${n}`).join(" ")
                : tr("（空）", "(empty)")}
            </span>
          </p>
        ))}
      </div>
    </div>
  );
}
