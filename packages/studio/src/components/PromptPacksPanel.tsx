import { useEffect, useMemo, useState } from "react";
// 子路径 import（组件惯例——barrel 会把 node 依赖模块拉进浏览器 bundle）。
import { diffChapterText, diffSummary, type TextDiffLine } from "@actalk/inkos-core/utils/text-diff";
import { fetchJson } from "../hooks/use-api";
import { tr } from "../lib/app-language";
import { ConfirmDialog } from "./ConfirmDialog";

/**
 * R54/1659 号：prompt pack 管理面（ProjectSettings 挂载）——prompt 基础面
 * 从零起：3 pack 12 prompt 列表（builtin/project 覆盖态）+ 内容编辑
 * （PUT 覆盖/DELETE 回 builtin）+ **版本历史**（1658 引擎端点
 * GET /prompt-packs/:id/versions）+ 行级 diff（core text-diff 复用=R53 面
 * 同形态）+ **一键回滚**（POST .../versions/restore——回滚本身也记版本行）。
 * regressionHint=prompt-changed → 评测提示（v8「知识接入先测」纪律通道）。
 * 端点=Rust 引擎腿（产品主路径 164 号）；Node 回退腿缺该族端点→空态降级
 * 文案（TS 腿补齐挂下轮，builtin 12 条单源=export-bindings 绑定生成）。
 */

interface PackPrompt {
  readonly id: string;
  readonly title: string;
  readonly source: "builtin" | "project" | "user" | "external";
  readonly overridden: boolean;
  readonly content: string;
  readonly defaultContent: string;
}

interface PromptPack {
  readonly id: string;
  readonly title: string;
  readonly description: string;
  readonly prompts: ReadonlyArray<string>;
}

interface VersionRow {
  readonly ts: string;
  readonly action: string;
  readonly preview: string;
  readonly content: string;
}

export function PromptPacksPanel() {
  const [packs, setPacks] = useState<ReadonlyArray<PromptPack> | null>(null);
  const [prompts, setPrompts] = useState<ReadonlyArray<PackPrompt>>([]);
  const [selectedId, setSelectedId] = useState("");
  const [draft, setDraft] = useState("");
  const [versions, setVersions] = useState<ReadonlyArray<VersionRow> | null>(null);
  const [diff, setDiff] = useState<{ versionTs: string; lines: ReadonlyArray<TextDiffLine> } | null>(null);
  const [busy, setBusy] = useState(false);
  // 1666 号：提示带形态——失败与成功此前同形（同样的灰色 status），读屏也
  // 一律当状态播报，「保存失败」和「已保存」在界面上分不出来。
  const [notice, setNotice] = useState<{ kind: "ok" | "error"; text: string } | null>(null);
  const [pendingReset, setPendingReset] = useState(false);
  const [pendingRestore, setPendingRestore] = useState<VersionRow | null>(null);

  const selected = useMemo(
    () => prompts.find((prompt) => prompt.id === selectedId) ?? null,
    [prompts, selectedId],
  );

  const reload = async () => {
    try {
      const payload = await fetchJson<{ packs: ReadonlyArray<PromptPack>; prompts: ReadonlyArray<PackPrompt> }>(
        "/prompt-packs",
      );
      setPacks(payload.packs ?? []);
      setPrompts(payload.prompts ?? []);
    } catch {
      setPacks([]);
    }
  };

  const reloadVersions = async (promptId: string) => {
    try {
      const payload = await fetchJson<{ versions: ReadonlyArray<VersionRow> }>(
        `/prompt-packs/${encodeURIComponent(promptId)}/versions`,
      );
      setVersions(payload.versions ?? []);
    } catch {
      setVersions([]);
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  useEffect(() => {
    setDraft(selected?.content ?? "");
    setDiff(null);
    setNotice(null);
    if (selectedId) void reloadVersions(selectedId);
    else setVersions(null);
  }, [selectedId, selected?.content]);

  const save = async () => {
    if (!selectedId) return;
    setBusy(true);
    setNotice(null);
    try {
      const result = await fetchJson<{ regressionHint?: string }>(
        `/prompt-packs/${encodeURIComponent(selectedId)}`,
        { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ content: draft }) },
      );
      await reload();
      await reloadVersions(selectedId);
      setNotice({
        kind: "ok",
        text: result?.regressionHint === "prompt-changed"
          ? tr("已保存——prompt 已变更，建议跑一轮行为评测确认无回归。", "Saved — prompt changed; consider running a behavior eval to check for regressions.")
          : tr("已保存。", "Saved."),
      });
    } catch (cause) {
      setNotice({ kind: "error", text: cause instanceof Error ? cause.message : tr("保存失败", "Save failed") });
    } finally {
      setBusy(false);
    }
  };

  const resetToBuiltin = async () => {
    if (!selectedId) return;
    setPendingReset(false);
    setBusy(true);
    try {
      await fetchJson(`/prompt-packs/${encodeURIComponent(selectedId)}`, { method: "DELETE" });
      await reload();
      await reloadVersions(selectedId);
      setNotice({ kind: "ok", text: tr("已恢复 builtin 默认。", "Restored builtin default.") });
    } catch (cause) {
      setNotice({ kind: "error", text: cause instanceof Error ? cause.message : tr("恢复失败", "Reset failed") });
    } finally {
      setBusy(false);
    }
  };

  const restore = async (row: VersionRow) => {
    if (!selectedId) return;
    setPendingRestore(null);
    setBusy(true);
    try {
      await fetchJson(`/prompt-packs/${encodeURIComponent(selectedId)}/versions/restore`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ts: row.ts }),
      });
      await reload();
      await reloadVersions(selectedId);
      setDiff(null);
      setNotice({ kind: "ok", text: tr("已回滚到所选版本（回滚本身也记入历史）。", "Rolled back to the selected version (the rollback itself is recorded).") });
    } catch (cause) {
      setNotice({ kind: "error", text: cause instanceof Error ? cause.message : tr("回滚失败", "Restore failed") });
    } finally {
      setBusy(false);
    }
  };

  if (packs === null) {
    return <p className="text-xs text-muted-foreground italic">{tr("prompt 面载入中…", "Loading prompts…")}</p>;
  }
  if (packs.length === 0) {
    return (
      <p className="text-xs text-muted-foreground italic" data-prompt-packs="unavailable">
        {tr("prompt-packs 端点不可达（Node 回退腿暂缺该域——引擎端可用）。", "prompt-packs endpoint unavailable (missing on the Node fallback leg; available on the engine).")}
      </p>
    );
  }

  return (
    <div className="space-y-3" data-prompt-packs="ready">
      <div className="grid gap-2 md:grid-cols-3">
        {packs.map((pack) => (
          <div key={pack.id} className="rounded-xl border border-border/60 bg-secondary/20 p-3">
            <div className="text-sm font-bold">{pack.title}</div>
            <p className="mt-1 text-[11px] text-muted-foreground">{pack.description}</p>
            <div className="mt-2 flex flex-col gap-1">
              {pack.prompts.map((promptId) => {
                const prompt = prompts.find((entry) => entry.id === promptId);
                return (
                  <button
                    key={promptId}
                    type="button"
                    disabled={busy}
                    onClick={() => setSelectedId(promptId)}
                    data-prompt-item={promptId}
                    className={`text-left text-xs rounded-lg px-2 py-1.5 transition disabled:opacity-40 disabled:hover:bg-transparent ${
                      selectedId === promptId
                        ? "bg-primary/15 text-primary font-bold"
                        : "text-muted-foreground hover:bg-secondary hover:text-foreground"
                    }`}
                  >
                    {prompt?.title ?? promptId}
                    {prompt?.overridden ? (
                      <span className="ml-1.5 rounded border border-primary/30 px-1 text-[9px] text-primary">
                        {tr("已覆盖", "overridden")}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>

      {selected ? (
        <div className="space-y-2 rounded-xl border border-border/60 bg-background p-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-sm font-bold">{selected.title}</span>
            <div className="flex items-center gap-2">
              <span className="text-[11px] text-muted-foreground">
                {selected.source === "project" ? tr("project 覆盖生效中", "project override active") : tr("builtin 默认", "builtin default")}
              </span>
              <button
                type="button"
                disabled={busy || !selected.overridden}
                onClick={() => setPendingReset(true)}
                data-prompt-action="reset"
                className="rounded-lg border border-border/60 px-3 py-1.5 text-xs font-bold text-muted-foreground transition hover:text-primary disabled:opacity-40"
              >
                {tr("恢复 builtin", "Reset to builtin")}
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => void save()}
                data-prompt-action="save"
                className="rounded-lg bg-primary px-3 py-1.5 text-xs font-bold text-primary-foreground disabled:opacity-40"
              >
                {busy ? tr("保存中…", "Saving…") : tr("保存覆盖", "Save override")}
              </button>
            </div>
          </div>
          {/* 1666 号：操作进行中锁定选择与编辑——保存/回滚的收尾会按发起时的
              prompt 刷新版本历史与草稿，中途切走会把旧 prompt 的历史挂到新选择
              底下、把保存期间的后续输入冲掉。 */}
          <textarea
            value={draft}
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            rows={10}
            data-prompt-editor={selected.id}
            className="w-full rounded-xl border border-border/60 bg-background px-3 py-2 font-mono text-xs leading-5 text-foreground outline-none focus:border-primary/40"
          />
          {notice ? (
            <p
              role={notice.kind === "error" ? "alert" : "status"}
              data-prompt-notice
              data-prompt-notice-kind={notice.kind}
              className={`text-[11px] ${notice.kind === "error" ? "text-destructive" : "text-muted-foreground"}`}
            >
              {notice.text}
            </p>
          ) : null}
        </div>
      ) : (
        <p className="text-xs text-muted-foreground italic">{tr("选择一个 prompt 开始编辑。", "Select a prompt to edit.")}</p>
      )}

      {selected && versions !== null ? (
        <div className="space-y-2 rounded-xl border border-border/60 bg-background p-3">
          <div className="text-sm font-bold">{tr("版本历史", "Version history")}</div>
          {versions.length === 0 ? (
            <p className="text-xs text-muted-foreground italic">{tr("暂无版本——保存覆盖后开始记录。", "No versions yet — recorded after you save an override.")}</p>
          ) : (
            <div className="space-y-1.5" data-prompt-versions>
              {versions.map((row) => (
                <div key={row.ts} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border/50 px-3 py-1.5">
                  <div className="min-w-0 text-[11px] text-muted-foreground">
                    <span className="font-bold text-foreground">{row.action}</span>
                    <span className="mx-2 text-border">·</span>
                    {row.ts}
                    <span className="mx-2 text-border">·</span>
                    <span className="truncate">{row.preview.slice(0, 40) || "—"}</span>
                  </div>
                  <div className="flex gap-2">
                    <button
                      type="button"
                      onClick={() => setDiff({
                        versionTs: row.ts,
                        lines: diffChapterText(row.content, selected.content),
                      })}
                      data-prompt-version-diff={row.ts}
                      className="rounded-lg px-2.5 py-1 text-[11px] font-bold text-muted-foreground transition hover:bg-secondary hover:text-foreground"
                    >
                      {tr("diff", "diff")}
                    </button>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => setPendingRestore(row)}
                      data-prompt-version-restore={row.ts}
                      className="rounded-lg px-2.5 py-1 text-[11px] font-bold text-primary transition hover:bg-primary/10 disabled:opacity-40"
                    >
                      {tr("回滚到此版", "Restore")}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
          {diff ? (
            <div className="space-y-1" data-prompt-diff={diff.versionTs}>
              <p className="text-[11px] text-muted-foreground">
                {tr("版本 vs 当前", "Version vs current")} · {(() => {
                  const s = diffSummary(diff.lines);
                  return `${s.kept} kept · ${s.removed} − · ${s.added} +`;
                })()}
              </p>
              <div className="max-h-64 overflow-auto rounded-lg border border-border/50 p-2 font-mono text-[11px] leading-5">
                {diff.lines.map((line, index) => (
                  <div
                    key={index}
                    data-diff-kind={line.kind}
                    className={
                      line.kind === "removed"
                        ? "bg-destructive/10 text-destructive px-1.5 rounded"
                        : line.kind === "added"
                          ? "bg-primary/10 text-primary px-1.5 rounded"
                          : "text-muted-foreground px-1.5"
                    }
                  >
                    {line.kind === "removed" ? "− " : line.kind === "added" ? "+ " : "  "}{line.text}
                  </div>
                ))}
              </div>
            </div>
          ) : null}
        </div>
      ) : null}

      <ConfirmDialog
        open={pendingReset}
        title={tr("恢复 builtin", "Reset to builtin")}
        message={tr("将移除 project 覆盖并回到 builtin 默认（删除也记入版本历史）。", "This removes the project override and returns to the builtin default (the deletion is recorded in history).")}
        confirmLabel={tr("恢复 builtin", "Reset to builtin")}
        cancelLabel={tr("取消", "Cancel")}
        onConfirm={() => void resetToBuiltin()}
        onCancel={() => setPendingReset(false)}
      />
      <ConfirmDialog
        open={pendingRestore !== null}
        title={tr("回滚到此版", "Restore version")}
        message={pendingRestore
          ? tr(`回滚到 ${pendingRestore.ts}（${pendingRestore.action}）——回滚本身也会记入版本历史。`, `Roll back to ${pendingRestore.ts} (${pendingRestore.action}) — the rollback itself is recorded in history.`)
          : ""}
        confirmLabel={tr("回滚", "Restore")}
        cancelLabel={tr("取消", "Cancel")}
        onConfirm={() => { if (pendingRestore !== null) void restore(pendingRestore); }}
        onCancel={() => setPendingRestore(null)}
      />
    </div>
  );
}
