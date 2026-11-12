import type { SSEMessage } from "../../hooks/use-sse";
import { Loader2, Check } from "lucide-react";
import { cn } from "../../lib/utils";
import { tr } from "../../lib/app-language";
import { SidebarCard } from "./SidebarCard";

// 每个步骤的 zh 文案同时也是与后台 SSE log 消息匹配的键（后台目前发中文消息）。
// 展示时按当前语言取 zh/en，匹配时 zh、en 都认，后台消息以后双语化也不用改这里。
interface ProgressStep {
  readonly zh: string;
  readonly en: string;
}

const INIT_BOOK_STEPS: ReadonlyArray<ProgressStep> = [
  { zh: "生成基础设定", en: "Generate foundation" },
  { zh: "保存书籍配置", en: "Save book config" },
  { zh: "写入基础设定文件", en: "Write foundation files" },
  { zh: "初始化控制文档", en: "Initialize control docs" },
  { zh: "创建初始快照", en: "Create initial snapshot" },
];

const WRITE_CHAPTER_STEPS: ReadonlyArray<ProgressStep> = [
  { zh: "准备章节输入", en: "Prepare chapter input" },
  { zh: "撰写章节草稿", en: "Draft the chapter" },
  { zh: "落盘最终章节", en: "Save final chapter" },
  { zh: "生成最终真相文件", en: "Generate final truth files" },
  { zh: "校验真相文件变更", en: "Validate truth file changes" },
  { zh: "同步记忆索引", en: "Sync memory index" },
  { zh: "更新章节索引与快照", en: "Update chapter index and snapshot" },
];

type StepStatus = "pending" | "active" | "done";

// 999 号：进度态全量重放（消除增量累积形态）——本组件同挂 ContextDock 的
// visible 条件渲染下（996/998 同族第三面）：dock 开合卸载期间 sse.messages
// 在 store 继续累积，旧增量形 effect 重挂载后只看最后一条，中间 log 步骤的
// 完成态全部丢失（进度条回退显示不完整）。进度态本就是消息流的纯派生——
// 全量重放幂等且卸载期事件不丢；单次写作 log 数十条，O(n) 重放远优于易丢
// 增量态。
interface DerivedProgress {
  readonly operation: "idle" | "init" | "write";
  readonly completedSteps: ReadonlySet<string>;
  readonly activeStep: string | null;
}

export function deriveProgressState(messages: ReadonlyArray<SSEMessage>): DerivedProgress {
  let operation: DerivedProgress["operation"] = "idle";
  let completedSteps = new Set<string>();
  let activeStep: string | null = null;

  for (const last of messages) {
    if (last.event === "book:creating") {
      operation = "init";
      completedSteps = new Set();
      activeStep = null;
    } else if (last.event === "write:start") {
      operation = "write";
      completedSteps = new Set();
      activeStep = null;
    } else if (last.event === "book:created" || last.event === "write:complete") {
      // Mark all steps done (the set stores zh keys / raw backend messages)
      const steps = operation === "init" ? INIT_BOOK_STEPS : WRITE_CHAPTER_STEPS;
      completedSteps = new Set(steps.map((s) => s.zh));
      activeStep = null;
    } else if (last.event === "log") {
      const data = last.data as { message?: string } | null;
      const message = data?.message;
      if (message && operation !== "idle") {
        // Mark previous active step as done, set new active
        if (activeStep) completedSteps.add(activeStep);
        activeStep = message;
      }
    }
  }
  return { operation, completedSteps, activeStep };
}

interface ProgressSectionProps {
  readonly sse: { messages: ReadonlyArray<SSEMessage>; connected: boolean };
}

export function ProgressSection({ sse }: ProgressSectionProps) {
  // 999 号：消息流全量重放派生（原三段增量 useState 形卸载期丢中间 log 步骤）
  const { operation, completedSteps, activeStep } = deriveProgressState(sse.messages);

  const steps = operation === "init" ? INIT_BOOK_STEPS
    : operation === "write" ? WRITE_CHAPTER_STEPS
    : null;

  if (!steps) return null;

  return (
    <SidebarCard title={tr("执行", "Progress")}>
      <ul className="space-y-2">
        {steps.map((step, i) => {
          const status: StepStatus =
            completedSteps.has(step.zh) || completedSteps.has(step.en) ? "done"
            : activeStep === step.zh || activeStep === step.en ? "active"
            : "pending";
          return (
            <li key={step.zh} className="flex items-center gap-2.5">
              <StepIndicator index={i + 1} status={status} />
              <span className={cn(
                "text-xs",
                status === "done" && "text-muted-foreground",
                status === "active" && "text-foreground font-medium",
                status === "pending" && "text-muted-foreground/50",
              )}>
                {tr(step.zh, step.en)}
              </span>
            </li>
          );
        })}
      </ul>
    </SidebarCard>
  );
}

function StepIndicator({ index, status }: { readonly index: number; readonly status: StepStatus }) {
  if (status === "done") {
    return (
      <div className="w-5 h-5 rounded-full bg-primary flex items-center justify-center shrink-0">
        <Check size={12} className="text-primary-foreground" strokeWidth={2.5} />
      </div>
    );
  }
  if (status === "active") {
    return (
      <div className="w-5 h-5 rounded-full border-2 border-primary flex items-center justify-center shrink-0">
        <Loader2 size={10} className="text-primary animate-spin" />
      </div>
    );
  }
  return (
    <div className="w-5 h-5 rounded-full border border-border/60 flex items-center justify-center shrink-0">
      <span className="text-[10px] text-muted-foreground/50">{index}</span>
    </div>
  );
}
