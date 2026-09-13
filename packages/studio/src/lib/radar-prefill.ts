/**
 * R27 雷达→一键开书（v5 五轮规划 P1，402 号）。
 *
 * 雷达推荐卡「从该选题开书」→ 建书对话流（ChatPage book-create 模式）输入框
 * 预填。纯 UI 编排：复用既有建书链（bookCreate 意图 → agent 确认流），
 * 不新增端点。文本是给建书 agent 的开局草稿——用户可改后再发送。
 */

export interface RadarPrefillSource {
  readonly platform: string;
  readonly genre: string;
  readonly concept: string;
}

/** 项目界面语言（zh → 中文草稿；en/其余 → 英文草稿）。 */
export function buildBookCreatePrefill(source: RadarPrefillSource, isZh: boolean): string {
  const platform = source.platform.trim();
  const genre = source.genre.trim();
  const concept = source.concept.trim();
  if (isZh) {
    return [
      "帮我建一本新书。",
      platform ? `目标平台：${platform}。` : "",
      genre ? `题材：${genre}。` : "",
      concept ? `核心设定：${concept}` : "",
    ].filter(Boolean).join("");
  }
  return [
    "Help me create a new book.",
    platform ? `Target platform: ${platform}.` : "",
    genre ? `Genre: ${genre}.` : "",
    concept ? `Core premise: ${concept}` : "",
  ].filter(Boolean).join(" ");
}
