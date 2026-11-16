import { useEffect, useRef, useState } from "react";
import { BANNER_TEXT, useEngineHealth } from "@/hooks/use-engine-health";

// 1463：引擎连接横幅——down 态固定顶条（role=alert 无障碍播报）；恢复沿
// 短 toast（3s 自消）。checking 态零渲染（首探 2s 内无打扰）。
export function EngineStatusBanner() {
  const health = useEngineHealth();
  const [toast, setToast] = useState(false);

  // 1469：恢复沿改为 health 派生（useEffect 监听 down→up 转变）——原
  // 形态经 hook onRecover 回调（async 轮询 tick 内直调），jsdom 行为测
  // 全绿但真浏览器（IAB/Chromium+RUM fetch 包装层）恢复沿 toast 从未
  // 渲染（MutationObserver 回溯零命中）；派生形态在 React 数据流内
  // commit 后必达，不依赖 async 回调链路。
  const prevHealthRef = useRef(health);
  useEffect(() => {
    if (prevHealthRef.current === "down" && health === "up") {
      setToast(true);
    }
    prevHealthRef.current = health;
  }, [health]);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(false), 3000);
    return () => clearTimeout(t);
  }, [toast]);

  if (health === "down") {
    return (
      <div
        role="alert"
        aria-live="assertive"
        style={{
          position: "fixed",
          top: 0,
          left: 0,
          right: 0,
          zIndex: 9999,
          background: "var(--color-danger, #b3261e)",
          color: "#fff",
          fontSize: 13,
          lineHeight: "32px",
          textAlign: "center",
          boxShadow: "0 1px 6px rgba(0,0,0,0.25)",
        }}
      >
        {BANNER_TEXT.down}
      </div>
    );
  }
  if (toast) {
    return (
      <div
        role="status"
        style={{
          position: "fixed",
          top: 12,
          left: "50%",
          transform: "translateX(-50%)",
          zIndex: 9999,
          background: "var(--color-surface-raised, #2e7d32)",
          color: "#fff",
          fontSize: 13,
          lineHeight: "28px",
          padding: "0 16px",
          borderRadius: 14,
          boxShadow: "0 1px 6px rgba(0,0,0,0.25)",
        }}
      >
        {BANNER_TEXT.recovered}
      </div>
    );
  }
  return null;
}
