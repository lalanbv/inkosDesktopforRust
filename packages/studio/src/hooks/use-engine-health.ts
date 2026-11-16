import { useEffect, useRef, useState } from "react";
import { tr } from "@/lib/app-language";

// 1463：引擎连接监控——1453 报障面（引擎不在跑→SPA 各卡片红错无全局
// 信号）与 1460 自愈链（壳自动重启期间 webview 内用户零反馈）的 webview
// 内用户可读提示。
//
// 探测端点选型：GET /api/v1/profiles（双栈恒有轻端点）——**不用**
// /api/v1/health：它是 62 号刻意保留的 Rust 超集端点（Node 回退腿无此
// 路由），用它探测在回退形态会 404 常亮误报。profiles 200=活；网络错/
// 非 2xx=死——与壳侧 probe_once 同判活语义。

/** 监控状态：checking=首探未决；up=健康；down=连续失败判中断。 */
export type EngineHealthState = "checking" | "up" | "down";

/** 正常态轮询周期（低频——健康时只是保活哨兵）。 */
const HEALTHY_INTERVAL_MS = 30_000;
/** 失败态提频周期（快速确认死亡/捕捉恢复）。 */
const PROBE_INTERVAL_MS = 2_000;
/** 连续失败判中断阈值（≈6s 窗口，与壳侧 RESUPERVISE_FAILURE_THRESHOLD 同律）。 */
export const FAILURE_THRESHOLD = 3;

/**
 * 下一轮探测延迟（1466 抽纯函数回归锁）：**只要出现失败**（failures>0，
 * 含未达阈值的中间态）即提频 2s；failures=0 且健康才回 30s 低频。
 *
 * 1464 活体走查教训：首版把提频绑在「已判 down」上——中间态仍等 30s，
 * 3 连失败需 ≈90s（比设计慢 15 倍），「失败提频」意图从未生效且纯函数
 * 测试测不到（周期是接线层内联）——抽离后四格单测锁死防回归。
 */
export function nextProbeDelayMs(
  failures: number,
  state: EngineHealthState,
): number {
  return failures > 0 || state === "down" ? PROBE_INTERVAL_MS : HEALTHY_INTERVAL_MS;
}

/**
 * 状态机纯函数（可测核心）：喂入单次探测结果，产出下一状态与失败计数。
 * 恢复沿（down→up）单列返回，供横幅 toast 一次性消费。
 */
export function nextHealth(
  alive: boolean,
  failures: number,
): { state: EngineHealthState; failures: number; recovered: boolean } {
  if (alive) {
    return { state: "up", failures: 0, recovered: failures >= FAILURE_THRESHOLD };
  }
  const next = failures + 1;
  return {
    state: next >= FAILURE_THRESHOLD ? "down" : "up",
    failures: next,
    recovered: false,
  };
}

/** 单发探测：2xx=活。no-store 防 1455 族中间层缓存假活。 */
async function probeAlive(): Promise<boolean> {
  try {
    const res = await fetch("/api/v1/profiles", { cache: "no-store" });
    return res.ok;
  } catch {
    return false;
  }
}

/**
 * 引擎健康轮询 hook。周期自适应：健康 30s 低频；进入失败提频 2s 快捕
 * 死亡/恢复。返回状态供横幅渲染；`onRecover` 在 down→up 恢复沿触发一次。
 */
export function useEngineHealth(onRecover?: () => void): EngineHealthState {
  const [state, setState] = useState<EngineHealthState>("checking");
  const failuresRef = useRef(0);
  const stateRef = useRef<EngineHealthState>("checking");
  const onRecoverRef = useRef(onRecover);
  onRecoverRef.current = onRecover;

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    const tick = async () => {
      const alive = await probeAlive();
      if (cancelled) return;
      const { state: next, failures, recovered } = nextHealth(alive, failuresRef.current);
      failuresRef.current = failures;
      // 恢复沿只在 down→up 转变触发（recovered=此前已判 down）。
      if (recovered && stateRef.current === "down") {
        onRecoverRef.current?.();
      }
      stateRef.current = next;
      setState(next);
      // 自适应周期：决策见 nextProbeDelayMs（1466 抽离，四格单测锁定）。
      timer = setTimeout(tick, nextProbeDelayMs(failures, next));
    };

    timer = setTimeout(tick, PROBE_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, []);

  return state;
}

/** 横幅文案（导出供测试锚定）。 */
export const BANNER_TEXT = {
  down: tr("引擎连接中断，正在自动恢复…", "Engine connection lost — recovering automatically…"),
  recovered: tr("引擎连接已恢复", "Engine connection restored"),
};
