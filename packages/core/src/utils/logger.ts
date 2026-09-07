import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
// === Types ===

export type LogLevel = "debug" | "info" | "warn" | "error";

export interface LogEntry {
  readonly level: LogLevel;
  readonly tag: string;
  readonly message: string;
  readonly timestamp: string;
  readonly ctx?: Record<string, unknown>;
}

export interface LogSink {
  readonly write: (entry: LogEntry) => void;
}

export interface Logger {
  readonly debug: (msg: string, ctx?: Record<string, unknown>) => void;
  readonly info: (msg: string, ctx?: Record<string, unknown>) => void;
  readonly warn: (msg: string, ctx?: Record<string, unknown>) => void;
  readonly error: (msg: string, ctx?: Record<string, unknown>) => void;
  readonly child: (tag: string, extraCtx?: Record<string, unknown>) => Logger;
}

// === Level Ordering ===

const LEVEL_ORDER: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

// === ANSI Colors ===

const COLORS: Record<LogLevel, string> = {
  debug: "\x1b[90m", // gray
  info: "\x1b[36m",  // cyan
  warn: "\x1b[33m",  // yellow
  error: "\x1b[31m", // red
};
const RESET = "\x1b[0m";

// === Built-in Sinks ===

export function createStderrSink(options: {
  readonly minLevel?: LogLevel;
  readonly enableColors?: boolean;
}): LogSink {
  const minLevel = options.minLevel ?? "info";
  const enableColors = options.enableColors ?? (process.stderr.isTTY ?? false);
  const minOrder = LEVEL_ORDER[minLevel];

  return {
    write(entry: LogEntry): void {
      if (LEVEL_ORDER[entry.level] < minOrder) return;

      const levelTag = entry.level.toUpperCase().padEnd(5);
      const prefix = `[${entry.tag}]`;

      if (enableColors) {
        const color = COLORS[entry.level];
        process.stderr.write(
          `${color}${levelTag}${RESET} ${prefix} ${entry.message}\n`,
        );
      } else {
        process.stderr.write(`${levelTag} ${prefix} ${entry.message}\n`);
      }
    },
  };
}

export function createJsonLineSink(writable: NodeJS.WritableStream): LogSink {
  return {
    write(entry: LogEntry): void {
      writable.write(JSON.stringify(entry) + "\n");
    },
  };
}

export const nullSink: LogSink = {
  write(): void {},
};

// === File Sink（210 号） ===

/**
 * 项目 `inkos.log` 落盘 sink——`GET /api/v1/logs` 双端都读该文件，但此前
 * 零写入方（LogViewer/doctor 日志面空转）。JSON 行追加；失败静默（日志
 * 不阻断业务）。字段形态同 LogEntry（Rust 侧 utils/log_file.rs 对齐）。
 */
export function createFileSink(path: string): LogSink {
  return {
    write(entry: LogEntry): void {
      try {
        const dir = dirname(path);
        mkdirSync(dir, { recursive: true });
        appendFileSync(path, JSON.stringify(entry) + "\n", "utf-8");
      } catch {
        // 静默：日志写失败不得影响业务流
      }
    },
  };
}

// === Factory ===

export function createLogger(options: {
  readonly tag: string;
  readonly sinks: ReadonlyArray<LogSink>;
  readonly minLevel?: LogLevel;
  readonly baseCtx?: Record<string, unknown>;
}): Logger {
  const { tag, sinks, baseCtx } = options;

  function emit(level: LogLevel, msg: string, ctx?: Record<string, unknown>): void {
    const entry: LogEntry = {
      level,
      tag,
      message: msg,
      timestamp: new Date().toISOString(),
      ...(ctx || baseCtx
        ? { ctx: { ...baseCtx, ...ctx } }
        : {}),
    };
    for (const sink of sinks) {
      sink.write(entry);
    }
  }

  return {
    debug: (msg, ctx) => emit("debug", msg, ctx),
    info: (msg, ctx) => emit("info", msg, ctx),
    warn: (msg, ctx) => emit("warn", msg, ctx),
    error: (msg, ctx) => emit("error", msg, ctx),
    child(childTag, extraCtx) {
      return createLogger({
        tag: childTag,
        sinks,
        baseCtx: { ...baseCtx, ...extraCtx },
      });
    },
  };
}
