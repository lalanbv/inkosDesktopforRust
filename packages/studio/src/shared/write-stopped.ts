/**
 * 用户主动停止 write-next 的双端中性文案——端内单一事实源（649 号，647 备案
 * 收敛：此前 hooks 与 api/server.ts 各持一份内联字面量）。跨端与 Rust
 * write_next_route 字面同形（642 号契约），Rust 侧字面量由其测试锚定。
 */

export const WRITE_STOPPED_MESSAGE = "写作已按您的要求停止。";

/**
 * 用户主动停止属中性结果、不以失败呈现（188 号）。644 号走查实证：642 号把
 * 停止广播换成中性文案后，UI 侧遗留的 `includes("Operation aborted")` 判定
 * 不再命中——中性文案被红色失败分支包裹自相矛盾。判定同时认双端中性文案
 * 与遗留 abort 字面（兼容旧快照/旧广播形态）。
 */
export function isWriteStoppedMessage(error: string | null | undefined): boolean {
  if (!error) return false;
  return error.includes(WRITE_STOPPED_MESSAGE) || error.includes("Operation aborted");
}
