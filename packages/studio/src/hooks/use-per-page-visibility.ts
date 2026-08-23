import { useCallback, useState } from "react";

/**
 * 面板开合状态按页记忆（P3-4）：每个页面（dashboard/book/chat…）独立记录
 * dock / 底部面板的开合，切换页面互不干扰、返回还原。
 * 存储形态：单个 JSON map `{"book":true,"chat":false}`，键 `inkos:studio:<name>-visibility`。
 */

export function readStoredVisibilityMap(
  storage: Pick<Storage, "getItem"> | null | undefined,
  storageKey: string,
): Record<string, boolean> {
  const raw = storage?.getItem(storageKey);
  if (!raw) return {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    const out: Record<string, boolean> = {};
    for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof value === "boolean") out[key] = value;
    }
    return out;
  } catch {
    return {};
  }
}

export function usePerPageVisibility(storageKey: string, page: string, defaultValue = false): {
  visible: boolean;
  toggle: () => void;
  setVisible: (visible: boolean) => void;
} {
  const [map, setMap] = useState<Record<string, boolean>>(() => {
    if (typeof window === "undefined") return {};
    try {
      return readStoredVisibilityMap(window.localStorage, storageKey);
    } catch {
      return {};
    }
  });

  const persist = useCallback((next: Record<string, boolean>) => {
    try {
      window.localStorage.setItem(storageKey, JSON.stringify(next));
    } catch {
      // 私密模式等：会话内生效即可
    }
  }, [storageKey]);

  const setVisible = useCallback((visible: boolean) => {
    setMap((current) => {
      // 与当前有效值（含 defaultValue）相同时不触发存储写
      if ((current[page] ?? defaultValue) === visible) return current;
      const next = { ...current, [page]: visible };
      persist(next);
      return next;
    });
  }, [page, persist, defaultValue]);

  const toggle = useCallback(() => {
    setMap((current) => {
      // 翻转"有效"可见值：map 无记录时按 defaultValue 起算（否则首击会
      // 把默认开的面板写成 true，表现为"按了没反应"）
      const next = { ...current, [page]: !(current[page] ?? defaultValue) };
      persist(next);
      return next;
    });
  }, [page, persist, defaultValue]);

  return { visible: map[page] ?? defaultValue, toggle, setVisible };
}
