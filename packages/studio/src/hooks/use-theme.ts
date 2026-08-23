import { useEffect, useState } from "react";

export type Theme = "light" | "dark";
/** P2-3 起的三态偏好：显式浅色 / 显式深色 / 跟随系统（auto）。 */
export type ThemeMode = "light" | "dark" | "auto";

const THEME_STORAGE_KEY = "inkos:studio:theme";

interface ThemeStorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function getTimeBasedThemeForHour(hour: number): Theme {
  return hour >= 6 && hour < 18 ? "light" : "dark";
}

function getTimeBasedTheme(): Theme {
  return getTimeBasedThemeForHour(new Date().getHours());
}

function getThemeStorage(): ThemeStorageLike | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * 存储读取（三态）。null = 从未显式选择：保留旧版"按时段"默认行为，
 * 避免老用户升级后体感突变（151 号 P2-3 既定）。
 */
export function readStoredThemeMode(storage: Pick<ThemeStorageLike, "getItem"> | null | undefined): ThemeMode | null {
  const stored = storage?.getItem(THEME_STORAGE_KEY);
  return stored === "light" || stored === "dark" || stored === "auto" ? stored : null;
}

export interface ResolveThemeInput {
  readonly mode: ThemeMode | null;
  readonly systemPrefersDark: boolean;
  readonly hour: number;
}

/** 偏好 → 生效主题（纯函数）：auto 跟随系统；未选择按时段。 */
export function resolveEffectiveTheme(input: ResolveThemeInput): Theme {
  switch (input.mode) {
    case "light":
      return "light";
    case "dark":
      return "dark";
    case "auto":
      return input.systemPrefersDark ? "dark" : "light";
    default:
      return getTimeBasedThemeForHour(input.hour);
  }
}

/**
 * 顶栏主题按钮的三态循环：light → dark → auto → light。
 * 从未选择（null）时首击按当前生效主题取反，保持经典开关体感。
 */
export function cycleThemeMode(mode: ThemeMode | null, current: Theme): ThemeMode {
  if (mode === null) return current === "light" ? "dark" : "light";
  return mode === "light" ? "dark" : mode === "dark" ? "auto" : "light";
}

/** 系统深浅偏好；无 matchMedia 环境（SSR/测试）退化为按时段。 */
function readSystemPrefersDark(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) {
    return getTimeBasedTheme() === "dark";
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function useTheme() {
  const [mode, setModeState] = useState<ThemeMode | null>(() => readStoredThemeMode(getThemeStorage()));
  const [systemPrefersDark, setSystemPrefersDark] = useState<boolean>(readSystemPrefersDark);
  const [hour, setHour] = useState(() => new Date().getHours());

  // 未显式选择时保留旧版 60s 时段轮询（6/18 点翻转）；一旦选择即停。
  useEffect(() => {
    if (mode !== null) return;
    const timer = setInterval(() => setHour(new Date().getHours()), 60000);
    return () => clearInterval(timer);
  }, [mode]);

  // auto 档：跟随系统深浅并即时响应（替代一切轮询）。
  useEffect(() => {
    if (mode !== "auto" || typeof window === "undefined" || !window.matchMedia) return;
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (event: MediaQueryListEvent) => setSystemPrefersDark(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, [mode]);

  const theme = resolveEffectiveTheme({ mode, systemPrefersDark, hour });

  const setThemeMode = (nextMode: ThemeMode) => {
    const storage = getThemeStorage();
    try {
      storage?.setItem(THEME_STORAGE_KEY, nextMode);
    } catch {
      // Ignore storage failures and keep the in-memory preference for this session.
    }
    setModeState(nextMode);
    if (nextMode === "auto") {
      setSystemPrefersDark(readSystemPrefersDark());
    }
  };

  return { theme, mode, setThemeMode };
}
