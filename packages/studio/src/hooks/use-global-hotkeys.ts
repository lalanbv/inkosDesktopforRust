import { useEffect, useRef } from "react";

/**
 * 全局快捷键分发器（P2-4）：单一 keydown 监听 → parseCombo 归一 → 查注册表分发。
 * 取代 App 里 P1-5 的 ⌘K 临时 keydown；后续 Cmd+P/Cmd+B/Cmd+/ 等按同一注册表追加。
 */

export interface HotkeyDef {
  /** 归一组合串："mod+k"、"mod+shift+p"、"esc"。mod = Meta/Ctrl 按平台归一。 */
  combo: string;
  /** 分发目标：命令注册表条目 id（如 "nav.logs"）或应用级动作 id（如 "app.palette.toggle"）。 */
  commandId: string;
}

/** 修饰键本身按下不算组合。 */
const MODIFIER_KEYS = new Set(["Meta", "Ctrl", "Control", "Alt", "Shift"]);

/** 事件 key 到组合串键名的归一。 */
function normalizeKey(key: string): string {
  if (key === "Escape") return "esc";
  return key.toLowerCase();
}

/**
 * KeyboardEvent → 归一组合串（纯函数）。
 * 顺序恒为 mod+alt+shift+key；字母/数字/符号小写。修饰键单独按下返回 null。
 */
export function parseCombo(event: {
  readonly key: string;
  readonly metaKey: boolean;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly shiftKey: boolean;
}): string | null {
  if (MODIFIER_KEYS.has(event.key)) return null;
  const parts: string[] = [];
  // macOS 用 ⌘（Meta），其它平台 Ctrl——归一为 mod，同一注册表跨平台生效
  if (event.metaKey || event.ctrlKey) parts.push("mod");
  if (event.altKey) parts.push("alt");
  if (event.shiftKey) parts.push("shift");
  parts.push(normalizeKey(event.key));
  return parts.join("+");
}

/** 事件目标是否处于可编辑态（输入框/文本域/下拉/contenteditable）。 */
function isEditableTarget(target: unknown): boolean {
  if (typeof target !== "object" || target === null) return false;
  const el = target as { tagName?: unknown; isContentEditable?: unknown };
  const tag = typeof el.tagName === "string" ? el.tagName.toUpperCase() : "";
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable === true;
}

/**
 * 可编辑区域放行规则（纯函数）：普通按键交给输入框；mod 组合（⌘K/⌘P…）
 * 属全局命令，输入框内仍要拦截分发。
 */
export function shouldIgnore(target: unknown, combo: string): boolean {
  if (!isEditableTarget(target)) return false;
  return !combo.startsWith("mod+");
}

/**
 * 组合串 → 展示徽标（纯函数）：mod 按平台渲染 ⌘ / Ctrl，键名大写。
 * "mod+shift+p" → "⌘⇧P"（macOS）/ "Ctrl+Shift+P"（其它）。
 */
export function formatCombo(combo: string, isMac: boolean): string {
  const parts = combo.split("+");
  return parts
    .map((part) => {
      if (part === "mod") return isMac ? "⌘" : "Ctrl";
      if (part === "alt") return isMac ? "⌥" : "Alt";
      if (part === "shift") return isMac ? "⇧" : "Shift";
      return part.length === 1 ? part.toUpperCase() : part;
    })
    .join(isMac ? "" : "+");
}

export interface GlobalHotkeysOptions {
  readonly defs: ReadonlyArray<HotkeyDef>;
  readonly runCommand: (commandId: string) => void;
}

/**
 * 挂载一次的 keydown 分发。runCommand 经 ref 每渲染刷新，
 * 命令闭包不持有过期状态（151 号 P1-5 同款风险缓解）。
 */
export function useGlobalHotkeys(options: GlobalHotkeysOptions): void {
  const runRef = useRef(options.runCommand);
  runRef.current = options.runCommand;
  const defsRef = useRef(options.defs);
  defsRef.current = options.defs;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const combo = parseCombo(event);
      if (!combo) return;
      if (shouldIgnore(event.target, combo)) return;
      const def = defsRef.current.find((entry) => entry.combo === combo);
      if (!def) return;
      event.preventDefault();
      runRef.current(def.commandId);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
