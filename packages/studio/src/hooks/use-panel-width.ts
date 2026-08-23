import { useCallback, useEffect, useRef, useState } from "react";

/**
 * 侧面板宽度拖拽(P3-2):pointer 事件 180~400px clamp、localStorage 记忆、
 * 双击分隔条复位 260。Cmd+B 折叠(V2 下活动栏即图标条,折叠=隐藏面板)。
 */

export const PANEL_WIDTH_STORAGE_KEY = "inkos:studio:panel-width";
export const MIN_PANEL_WIDTH = 180;
export const MAX_PANEL_WIDTH = 400;
export const DEFAULT_PANEL_WIDTH = 260;

/** P3-4 起支持自定义边界/存储键（右侧 dock 280~700 独立记忆）与拖拽方向。 */
export interface PanelWidthOptions {
  min?: number;
  max?: number;
  defaultWidth?: number;
  storageKey?: string;
  /** 拖拽分隔条在面板哪一侧：left=面板贴视口左缘(默认)，right=贴右缘(dock)。 */
  side?: "left" | "right";
}

/** 纯函数:宽度钳制与非法值回退（边界可覆写，默认左侧面板 180~400）。 */
export function clampPanelWidth(
  px: number,
  min: number = MIN_PANEL_WIDTH,
  max: number = MAX_PANEL_WIDTH,
  fallback: number = DEFAULT_PANEL_WIDTH,
): number {
  if (!Number.isFinite(px)) return fallback;
  return Math.min(max, Math.max(min, Math.round(px)));
}

export function readStoredPanelWidth(
  storage: Pick<Storage, "getItem"> | null | undefined,
  storageKey: string = PANEL_WIDTH_STORAGE_KEY,
  min: number = MIN_PANEL_WIDTH,
  max: number = MAX_PANEL_WIDTH,
  fallback: number = DEFAULT_PANEL_WIDTH,
): number {
  const raw = Number(storage?.getItem(storageKey));
  if (!Number.isFinite(raw) || raw <= 0) return fallback;
  // 存量值也过一遍 clamp,保证范围约束向前兼容
  return clampPanelWidth(raw, min, max, fallback);
}

export function usePanelWidth(options: PanelWidthOptions = {}) {
  const { min, max, defaultWidth, storageKey, side = "left" } = options;
  const [width, setWidth] = useState(() => {
    const fallback = defaultWidth ?? DEFAULT_PANEL_WIDTH;
    if (typeof window === "undefined") return fallback;
    try {
      return readStoredPanelWidth(
        window.localStorage,
        storageKey ?? PANEL_WIDTH_STORAGE_KEY,
        min ?? MIN_PANEL_WIDTH,
        max ?? MAX_PANEL_WIDTH,
        fallback,
      );
    } catch {
      return fallback;
    }
  });
  const [hidden, setHidden] = useState(false);
  const draggingRef = useRef(false);

  const storageKeyRef = storageKey ?? PANEL_WIDTH_STORAGE_KEY;
  const bounds = { min: min ?? MIN_PANEL_WIDTH, max: max ?? MAX_PANEL_WIDTH, fallback: defaultWidth ?? DEFAULT_PANEL_WIDTH };
  const persist = useCallback((next: number) => {
    try {
      window.localStorage.setItem(storageKeyRef, String(next));
    } catch {
      // 私密模式等:会话内生效即可
    }
  }, [storageKeyRef]);

  // 分隔条 pointer 流:down 捕获 → move 钳制更新 → up 落盘
  const beginResize = useCallback((event: React.PointerEvent) => {
    if (event.button !== 0) return;
    draggingRef.current = true;
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }, []);

  const handleResizeMove = useCallback((event: React.PointerEvent) => {
    if (!draggingRef.current) return;
    // left: 面板贴视口左缘,指针 x 即新宽度;right: 面板贴右缘,取右余量
    const raw = side === "right" ? window.innerWidth - event.clientX : event.clientX;
    setWidth(clampPanelWidth(raw, bounds.min, bounds.max, bounds.fallback));
  }, [side, bounds.min, bounds.max, bounds.fallback]);

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
    setWidth(bounds.fallback);
    persist(bounds.fallback);
  }, [persist, bounds.fallback]);

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
