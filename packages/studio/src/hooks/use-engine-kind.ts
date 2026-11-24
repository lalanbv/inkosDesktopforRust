import { useEffect, useState } from "react";

// 1668：服务腿判型——产品引擎（Rust）是否在位。
//
// 用途：仅产品引擎执行的功能（反AI规则写作禁则/审查扫描、经验条目注入、
// best-of-N 多版选优——1667 号查实 2.0 core 只有纯函数引擎、零管线调用点）
// 在 Node 回退腿上如实标明「保存后不参与写作与审查」，不做空开关。
//
// 判据 = GET /api/v1/health：62 号刻意保留的 Rust 超集端点，Node 回退腿无此
// 路由（1663 起 duel 守卫判据同源）。200 = 产品引擎；404 = Node 回退腿；
// 其余（网络错 / 5xx / 其它状态）不下结论 = unknown——判错型比不标更糟。
//
// 与 use-engine-health 的分工：那边是**判活**（1463 明令不可用 health——回退
// 形态 404 会常亮误报）；这边是**判型**，404 恰是信号。

export type EngineKind = "checking" | "rust" | "node" | "unknown";
export type ResolvedEngineKind = Exclude<EngineKind, "checking">;

export async function probeEngineKind(
  fetchImpl: typeof fetch = fetch,
): Promise<ResolvedEngineKind> {
  try {
    const res = await fetchImpl("/api/v1/health", { cache: "no-store" });
    if (res.ok) return "rust";
    return res.status === 404 ? "node" : "unknown";
  } catch {
    return "unknown";
  }
}

// 腿型在一次页面生命周期内不变：整页共享一次探测，多个挂载点不各打一次。
let cached: Promise<ResolvedEngineKind> | null = null;

export function engineKind(): Promise<ResolvedEngineKind> {
  cached ??= probeEngineKind();
  return cached;
}

/** 仅供测试在用例间重置共享探测。 */
export function resetEngineKindCache(): void {
  cached = null;
}

export function useEngineKind(): EngineKind {
  const [kind, setKind] = useState<EngineKind>("checking");
  useEffect(() => {
    let cancelled = false;
    void engineKind().then((resolved) => {
      if (!cancelled) setKind(resolved);
    });
    return () => {
      cancelled = true;
    };
  }, []);
  return kind;
}
