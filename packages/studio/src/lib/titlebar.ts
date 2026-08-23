/**
 * 标题栏融合（P2-1）：平台 / Tauri 运行时判定与顶栏交通灯留白。
 * 纯函数入参注入（platform 字符串、全局对象），渲染层与测试都不依赖真实环境。
 */

/** macOS 判定：navigator.platform（如 "MacIntel"）含 Mac。 */
export function isMacPlatformAgent(platform: string): boolean {
  return /Mac/i.test(platform);
}

/** Tauri v2 壳内判定：注入的 __TAURI_INTERNALS__ 全局是否存在。 */
export function isTauriRuntime(globalScope: unknown): boolean {
  return typeof globalScope === "object" && globalScope !== null && "__TAURI_INTERNALS__" in globalScope;
}

/** macOS 交通灯（红黄绿）在 Overlay 标题栏下的横向占位像素数（151 号 P2-1 规格）。 */
export const TITLEBAR_INSET_PX = 78;

/**
 * 顶栏左内边距：仅 macOS 且 Tauri 壳内为交通灯预留 78px；
 * 浏览器开发模式与 Win/Linux（系统标题栏）维持常规 32px。
 * Tailwind 类名须静态字面量，故在此集中导出。
 */
export function deriveHeaderInsetClass(mac: boolean, tauri: boolean): string {
  return mac && tauri ? "pl-[78px]" : "pl-8";
}
