import {
  Zap,
  Square,
  Search,
  FileOutput,
  TrendingUp,
} from "lucide-react";

export interface QuickActionsProps {
  readonly onAction: (command: string, requestedIntent?: "write_next") => void;
  readonly disabled: boolean;
  readonly isZh: boolean;
  /** 648 号：write_next 生产任务执行中——「写下一章」chip 翻转为就地停止。 */
  readonly writeNextRunning?: boolean;
  /** 648 号：就地停止回调（复用聊天中止链 abortSession → /sessions/:id/abort，
   *  后端经 reservedProductionSessions→activeConfirmedTasks 命中任务控制器）。 */
  readonly onStopWriteNext?: () => void;
}

interface ChipDef {
  readonly icon: React.ReactNode;
  readonly stopIcon?: React.ReactNode;
  readonly labelZh: string;
  readonly labelEn: string;
  readonly stopLabelZh?: string;
  readonly stopLabelEn?: string;
  readonly commandZh: string;
  readonly commandEn: string;
  readonly requestedIntent?: "write_next";
}

const CHIPS: ReadonlyArray<ChipDef> = [
  {
    icon: <Zap size={12} />,
    stopIcon: <Square size={12} />,
    labelZh: "写下一章",
    labelEn: "Write next",
    stopLabelZh: "停止写作",
    stopLabelEn: "Stop writing",
    commandZh: "写下一章",
    commandEn: "write next",
    requestedIntent: "write_next",
  },
  {
    icon: <Search size={12} />,
    labelZh: "审计",
    labelEn: "Audit",
    commandZh: "审计",
    commandEn: "audit",
  },
  {
    icon: <FileOutput size={12} />,
    labelZh: "导出",
    labelEn: "Export",
    commandZh: "导出全书",
    commandEn: "export book",
  },
  {
    icon: <TrendingUp size={12} />,
    labelZh: "市场雷达",
    labelEn: "Market radar",
    commandZh: "扫描市场趋势",
    commandEn: "scan market trends",
  },
];

export function QuickActions({ onAction, disabled, isZh, writeNextRunning = false, onStopWriteNext }: QuickActionsProps) {
  return (
    <div className="flex gap-2 overflow-x-auto px-1 py-1">
      {CHIPS.map((chip) => {
        const running = writeNextRunning && chip.requestedIntent === "write_next" && Boolean(onStopWriteNext);
        const label = running
          ? (isZh ? chip.stopLabelZh! : chip.stopLabelEn!)
          : (isZh ? chip.labelZh : chip.labelEn);
        const command = isZh ? chip.commandZh : chip.commandEn;
        return (
          <button
            key={chip.labelEn}
            onClick={() => {
              if (running) {
                onStopWriteNext!();
                return;
              }
              onAction(command, chip.requestedIntent);
            }}
            disabled={disabled && !running}
            data-slot={running ? "quick-action-stop" : undefined}
            className={`shrink-0 flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all disabled:opacity-40 group ${
              running
                ? "bg-destructive text-destructive-foreground hover:scale-105 active:scale-95 shadow-destructive/20"
                : "bg-secondary/50 border border-border/30 text-muted-foreground hover:text-primary hover:border-primary/30 hover:bg-primary/5 disabled:pointer-events-none"
            }`}
          >
            <span className="group-hover:scale-110 transition-transform">{running ? chip.stopIcon : chip.icon}</span>
            {label}
          </button>
        );
      })}
    </div>
  );
}
