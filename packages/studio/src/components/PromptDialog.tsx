import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

interface PromptDialogProps {
  readonly open: boolean;
  readonly title: string;
  /** 输入框上方说明（可选）。 */
  readonly message?: string;
  readonly placeholder?: string;
  /** 初始值（改名场景回填当前名）。 */
  readonly defaultValue?: string;
  readonly confirmLabel: string;
  readonly cancelLabel: string;
  /** 多行文本（brief/context 输入面）；缺省单行。 */
  readonly multiline?: boolean;
  readonly onConfirm: (value: string) => void;
  readonly onCancel: () => void;
}

/**
 * 1650 号：文本输入对话框（React 弹层）。wry 0.57 的 WKUIDelegate 未实现
 * runJavaScriptTextInputPanelWithPrompt——桌面壳内 window.prompt 返回
 * null/抛不支持（IAB 实测 prompt() is not supported），卷新建/改名等输入
 * 面是死按钮。与 ConfirmDialog 同视觉语言，替换原生 prompt 依赖。
 */
export function PromptDialog({
  open,
  title,
  message,
  placeholder,
  defaultValue = "",
  confirmLabel,
  cancelLabel,
  multiline = false,
  onConfirm,
  onCancel,
}: PromptDialogProps) {
  const overlayRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement | HTMLTextAreaElement | null>(null);
  const [value, setValue] = useState(defaultValue);

  // 每次打开重置为 defaultValue 并聚焦全选（改名场景便于覆写）。
  useEffect(() => {
    if (!open) return;
    setValue(defaultValue);
    const timer = window.setTimeout(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    }, 30);
    return () => window.clearTimeout(timer);
  }, [open, defaultValue]);

  useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onCancel]);

  if (!open) return null;
  if (typeof document === "undefined") return null;

  const submit = () => onConfirm(value);

  return createPortal(
    <div
      ref={overlayRef}
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 backdrop-blur-sm fade-in"
      onClick={(e) => { if (e.target === overlayRef.current) onCancel(); }}
    >
      <div className="bg-card border border-border rounded-2xl shadow-2xl shadow-primary/10 w-full max-w-md mx-4 overflow-hidden chat-msg-assistant">
        {/* Header */}
        <div className="flex items-center justify-between px-6 pt-6 pb-2">
          <h3 className="text-lg font-semibold">{title}</h3>
          <button
            onClick={onCancel}
            className="p-1.5 rounded-lg text-muted-foreground hover:text-foreground hover:bg-secondary transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        {/* Body */}
        <div className="px-6 py-4 space-y-3">
          {message ? <p className="text-sm text-muted-foreground leading-relaxed">{message}</p> : null}
          {multiline ? (
            <textarea
              ref={inputRef as React.RefObject<HTMLTextAreaElement>}
              value={value}
              placeholder={placeholder}
              onChange={(e) => setValue(e.target.value)}
              rows={4}
              className="w-full rounded-xl border border-border/60 bg-background/60 px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          ) : (
            <input
              ref={inputRef as React.RefObject<HTMLInputElement>}
              value={value}
              placeholder={placeholder}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) submit(); }}
              className="w-full rounded-xl border border-border/60 bg-background/60 px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 px-6 pb-6">
          <button
            onClick={onCancel}
            className="px-4 py-2.5 text-sm font-medium rounded-xl bg-secondary text-foreground hover:bg-secondary/80 transition-all border border-border/50"
          >
            {cancelLabel}
          </button>
          <button
            onClick={submit}
            className="px-4 py-2.5 text-sm font-bold rounded-xl bg-primary text-primary-foreground transition-all hover:scale-105 active:scale-95 shadow-sm hover:shadow-primary/20"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
