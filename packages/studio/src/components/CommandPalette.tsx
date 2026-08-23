import { useEffect, useMemo, useState } from "react";
import {
  Command,
  CommandDialog,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import { tr } from "@/lib/app-language";
import {
  buildActionCommands,
  buildNavigationCommands,
  buildRecentCommands,
  buildRecommendedCommands,
  commandTitle,
  filterCommands,
  groupCommands,
} from "@/lib/commands";
import type {
  CommandContext,
  CommandEntry,
  CommandGroup as CommandGroupId,
  PaletteLang,
} from "@/lib/commands";
import { useRecentsStore } from "@/store/recents";
import {
  BookCopy,
  BookPlus,
  Boxes,
  Clapperboard,
  Feather,
  FileInput,
  Film,
  Gamepad2,
  GitBranch,
  History,
  House,
  Languages,
  MessageSquare,
  Monitor,
  Moon,
  Plus,
  RefreshCw,
  Rows3,
  ScrollText,
  Settings,
  Stethoscope,
  Sun,
  Terminal,
  TrendingUp,
  Wand2,
  Zap,
  Globe,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

/** CommandEntry.icon（lucide 名）→ 渲染组件。缺失的名静默不显示图标。 */
const COMMAND_ICONS: Record<string, LucideIcon> = {
  house: House,
  "message-square": MessageSquare,
  "book-plus": BookPlus,
  settings: Settings,
  zap: Zap,
  terminal: Terminal,
  boxes: Boxes,
  wand: Wand2,
  languages: Languages,
  "file-input": FileInput,
  "trending-up": TrendingUp,
  stethoscope: Stethoscope,
  feather: Feather,
  "book-copy": BookCopy,
  plus: Plus,
  scroll: ScrollText,
  clapperboard: Clapperboard,
  rows: Rows3,
  film: Film,
  "git-branch": GitBranch,
  gamepad: Gamepad2,
  moon: Moon,
  sun: Sun,
  monitor: Monitor,
  globe: Globe,
  refresh: RefreshCw,
  history: History,
};

export interface PaletteGroupView {
  key: string;
  label: string;
  entries: ReadonlyArray<CommandEntry>;
}

/**
 * 面板主体（不含 Dialog 壳）：单独导出以便 SSR 冒烟测试——Base UI Dialog
 * 走 portal，renderToString 不输出其内容。过滤在 lib/commands 完成，
 * cmdk 自带过滤关闭（shouldFilter={false}），避免 id 与中文查询不匹配。
 */
export function PaletteBody({ groups, lang, fallbackEntry, onRun, query, onQueryChange }: {
  groups: ReadonlyArray<PaletteGroupView>;
  lang: PaletteLang;
  /** 零匹配时的兜底动作（创建新书），保证面板永远不是死胡同。 */
  fallbackEntry: CommandEntry;
  onRun: (entry: CommandEntry) => void;
  query: string;
  onQueryChange: (query: string) => void;
}) {
  const totalEntries = groups.reduce((sum, group) => sum + group.entries.length, 0);

  return (
    <Command shouldFilter={false}>
      <CommandInput
        value={query}
        onValueChange={onQueryChange}
        placeholder={tr("输入命令或搜索…", "Type a command or search…")}
      />
      <CommandList>
        {totalEntries === 0 ? (
          <>
            <div data-slot="palette-empty" className="px-4 py-6 text-center">
              <div className="text-sm text-muted-foreground">
                {tr("没有匹配的命令", "No matching commands")}
              </div>
              <div className="mt-1 text-xs text-muted-foreground/70">
                {tr("试试其他关键词，或直接开始创作。", "Try another keyword, or start creating.")}
              </div>
            </div>
            <CommandGroup heading={tr("推荐", "Recommended")}>
              <CommandItem value={fallbackEntry.id} onSelect={() => onRun(fallbackEntry)}>
                <Plus size={14} className="shrink-0 text-muted-foreground" />
                <span>{tr("创建新书", "Create new book")}</span>
              </CommandItem>
            </CommandGroup>
          </>
        ) : (
          groups.map((group) =>
            group.entries.length === 0 ? null : (
              <CommandGroup key={group.key} heading={group.label}>
                {group.entries.map((entry) => {
                  const Icon = entry.icon ? COMMAND_ICONS[entry.icon] : undefined;
                  return (
                    <CommandItem key={entry.id} value={entry.id} onSelect={() => onRun(entry)}>
                      {Icon ? <Icon size={14} className="shrink-0 text-muted-foreground" /> : null}
                      <span className="truncate">{commandTitle(entry, lang)}</span>
                      {entry.hotkey ? <CommandShortcut>{entry.hotkey}</CommandShortcut> : null}
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            ),
          )
        )}
      </CommandList>
    </Command>
  );
}

const GROUP_LABELS: Record<CommandGroupId, () => string> = {
  recent: () => tr("最近访问", "Recent"),
  navigation: () => tr("前往", "Go to"),
  action: () => tr("操作", "Actions"),
};

/**
 * 全局命令面板（⌘K / Ctrl+K）：空查询显示最近访问 + 推荐；输入即过滤
 * 导航 / 动作 / 最近三类命令。Esc / ↑↓↵ 由 cmdk 与 Dialog 内建。
 */
export function CommandPalette({ open, onOpenChange, ctx, lang }: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** App 每次渲染重建，命令闭包不持有过期状态。 */
  ctx: CommandContext;
  lang: PaletteLang;
}) {
  const recents = useRecentsStore((state) => state.recents);
  const [query, setQuery] = useState("");

  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  const allCommands = useMemo(
    () => [...buildNavigationCommands(), ...buildActionCommands()],
    [],
  );
  const recentCommands = useMemo(() => buildRecentCommands(recents), [recents]);

  const trimmed = query.trim();
  const groups: PaletteGroupView[] = trimmed
    ? groupCommands(filterCommands([...recentCommands, ...allCommands], trimmed)).map((group) => ({
        key: group.group,
        label: GROUP_LABELS[group.group](),
        entries: group.entries,
      }))
    : [
        {
          key: "recent",
          label: GROUP_LABELS.recent(),
          entries: recentCommands.slice(0, 5),
        },
        {
          key: "recommended",
          label: tr("推荐", "Recommended"),
          entries: buildRecommendedCommands(allCommands, recentCommands),
        },
      ];

  const run = (entry: CommandEntry) => {
    onOpenChange(false);
    entry.run(ctx);
  };

  const fallbackEntry =
    allCommands.find((entry) => entry.id === "action.bookCreate") ?? allCommands[0];

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title={tr("命令面板", "Command Palette")}
      description={tr("搜索或输入命令…", "Search or type a command…")}
    >
      <PaletteBody
        groups={groups}
        lang={lang}
        fallbackEntry={fallbackEntry}
        onRun={run}
        query={query}
        onQueryChange={setQuery}
      />
    </CommandDialog>
  );
}
