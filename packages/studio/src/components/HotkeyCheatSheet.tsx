import { useMemo } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { tr } from "@/lib/app-language";
import { buildActionCommands, buildNavigationCommands, commandTitle } from "@/lib/commands";
import type { PaletteLang } from "@/lib/commands";
import { formatCombo } from "@/hooks/use-global-hotkeys";
import type { HotkeyDef } from "@/hooks/use-global-hotkeys";

/** 应用级动作 id → 双语标签（注册表之外的少量全局键）。 */
const APP_ACTION_LABELS: Record<string, { zh: string; en: string }> = {
  "app.palette.toggle": { zh: "打开/关闭命令面板", en: "Toggle command palette" },
  "app.quickopen.toggle": { zh: "快速打开（书/章节/会话）", en: "Quick open (books/chapters/sessions)" },
  "app.cheatsheet.toggle": { zh: "快捷键速查表", en: "Shortcut cheat sheet" },
};

/**
 * 快捷键速查弹层（P2-6）：数据源 = HotkeyDef 注册表 + 命令表 hotkey 字段，
 * 不维护第二份手写清单——注册表加键，速查页自动出现。
 */
export function HotkeyCheatSheet({ open, onOpenChange, defs, lang, isMac }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  defs: ReadonlyArray<HotkeyDef>;
  lang: PaletteLang;
  isMac: boolean;
}) {
  const rows = useMemo(() => {
    const commandRows = [
      ...buildNavigationCommands(),
      ...buildActionCommands(),
    ]
      .filter((entry) => entry.hotkey)
      .map((entry) => ({
        key: `cmd:${entry.id}`,
        badge: entry.hotkey!,
        label: commandTitle(entry, lang),
      }));
    const defRows = defs.map((def) => {
      const appLabel = APP_ACTION_LABELS[def.commandId];
      return {
        key: `def:${def.combo}`,
        badge: formatCombo(def.combo, isMac),
        label: appLabel
          ? (lang === "en" ? appLabel.en : appLabel.zh)
          : def.commandId,
      };
    });
    return [...defRows, ...commandRows];
  }, [defs, isMac, lang]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" data-slot="hotkey-cheatsheet">
        <DialogHeader>
          <DialogTitle>{tr("快捷键速查", "Keyboard Shortcuts")}</DialogTitle>
          <DialogDescription>
            {tr("与命令面板注册表同源，新快捷键会自动出现在这里。", "Generated from the hotkey registry — new bindings appear here automatically.")}
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-72 overflow-y-auto">
          <table className="w-full text-sm">
            <tbody>
              {rows.map((row) => (
                <tr key={row.key} className="border-b border-border/40 last:border-0">
                  <td className="py-2 pr-4 text-muted-foreground">{row.label}</td>
                  <td className="py-2 text-right">
                    <kbd className="rounded border border-border/60 bg-muted/60 px-1.5 py-0.5 font-mono text-xs text-foreground">
                      {row.badge}
                    </kbd>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </DialogContent>
    </Dialog>
  );
}
