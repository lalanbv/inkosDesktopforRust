import { useCallback, useEffect, useRef, useState } from "react";

/**
 * 侧面板宽度拖拽(P3-2):pointer 事件 180~400px clamp、localStorage 记忆、
 * 双击分隔条复位 260。Cmd+B 折叠(V2 下活动栏即图标条,折叠=隐藏面板)。
 */

export const PANEL_WIDTH_STORAGE_KEY = "inkos:studio:panel-width";
export const MIN_PANEL_WIDTH = 180;
export const MAX_PANEL_WIDTH = 400;
export const DEFAULT_PANEL_WIDTH = 260;

/** 纯函数:宽度钳制与非法值回退。 */
export function clampPanelWidth(px: number): number {
  if (!Number.isFinite(px)) return DEFAULT_PANEL_WIDTH;
  return Math.min(MAX_PANEL_WIDTH, Math.max(MIN_PANEL_WIDTH, Math.round(px)));
}

export function readStoredPanelWidth(storage: Pick<Storage, "getItem"> | null | undefined): number {
  const raw = Number(storage?.getItem(PANEL_WIDTH_STORAGE_KEY));
  if (!Number.isFinite(raw) || raw <= 0) return DEFAULT_PANEL_WIDTH;
  // 存量值也过一遍 clamp,保证范围约束向前兼容
  return clampPanelWidth(raw);
}

export function usePanelWidth() {
  const [width, setWidth] = useState(() => {
    if (typeof window === "undefined") return DEFAULT_PANEL_WIDTH;
    try {
      return readStoredPanelWidth(window.localStorage);
    } catch {
      return DEFAULT_PANEL_WIDTH;
    }
  });
  const [hidden, setHidden] = useState(false);
  const draggingRef = useRef(false);

  const persist = useCallback((next: number) => {
    try {
      window.localStorage.setItem(PANEL_WIDTH_STORAGE_KEY, String(next));
    } catch {
      // 私密模式等:会话内生效即可
    }
  }, []);

  // 分隔条 pointer 流:down 捕获 → move 钳制更新 → up 落盘
  const beginResize = useCallback((event: React.PointerEvent) => {
    if (event.button !== 0) return;
    draggingRef.current = true;
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }, []);

  const handleResizeMove = useCallback((event: React.PointerEvent) => {
    if (!draggingRef.current) return;
    // 面板从视口左侧起算:指针 x 即新宽度
    setWidth(clampPanelWidth(event.clientX));
  }, []);

  const endResize = useCallback((event: React.PointerEvent) => {
    if (!draggingRef.current) return;
    draggingRef.current = false;
    (event.target as HTMLElement).releasePointerCapture(event.pointerId);
    setWidth((current) => {
      persist(current);
      return current;
    });
  }, [persist]);

  const resetWidth = useCallback(() => {
    setWidth(DEFAULT_PANEL_WIDTH);
    persist(DEFAULT_PANEL_WIDTH);
  }, [persist]);

  // 兜底:指针意外丢失(切窗等)时结束拖拽并落盘
  useEffect(() => {
    const onLostPointerCapture = () => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      setWidth((current) => {
        persist(current);
        return current;
      });
    };
    window.addEventListener("lostpointercapture", onLostPointerCapture);
    return () => window.removeEventListener("lostpointercapture", onLostPointerCapture);
  }, [persist]);

  const toggleHidden = useCallback(() => setHidden((value) => !value), []);

  return {
    width,
    hidden,
    beginResize,
    handleResizeMove,
    endResize,
    resetWidth,
    toggleHidden,
  };
}
