import { useCallback } from "react";
import { usePreferencesStore } from "@/store/preferences";

/**
 * 专注模式（P4-1）：进入=应用根节点加 .focus-mode（CSS 隐藏导航 chrome +
 * 低对比环境变量），退出=Esc / ⌘⇧F（App 分发）。偏好持久化（显式 true 才恢复）。
 * reduced-motion 用户进入时滚动平滑自动关闭（P4-2 的全局媒体查询兜底）。
 */
export function useFocusMode(): {
  focusMode: boolean;
  enter: () => void;
  exit: () => void;
  toggle: () => void;
} {
  const focusMode = usePreferencesStore((state) => state.focusMode);
  const setFocusMode = usePreferencesStore((state) => state.setFocusMode);

  const enter = useCallback(() => setFocusMode(true), [setFocusMode]);
  const exit = useCallback(() => setFocusMode(false), [setFocusMode]);
  const toggle = useCallback(() => setFocusMode(!focusMode), [focusMode, setFocusMode]);

  return { focusMode, enter, exit, toggle };
}
