/**
 * R11 场景节拍驱动写作（378 号契约批，三轮 P0；v3 §3 R11）。
 *
 * 章内 scene 粒度节拍：planner 产出场景节拍列表（每场景 title/description/
 * exitHook），writer 按节拍顺序推进生成散文。对齐 Novelcrafter Scene Beats
 * 的「节拍短描述 → AI 生成正文」工作流。
 *
 * 开关：`book.writing.sceneBeats`（默认关——关闭时 planner/writer 行为与
 * 既有链路逐字节一致）。接线批（379 号）挂 planner/writer 调用点。
 *
 * 双端：`engine-rs/src/utils/scene_beats.rs` 1:1 镜像；共享向量
 * `src/__tests__/golden/scene-beats-vectors.json`（TS 断言
 * `golden-scene-beats.test.ts`；Rust 差分 `tests/golden_scene_beats_diff.rs`）。
 */

export interface SceneBeat {
  readonly id: string;
  readonly title: string;
  /** 场景内容描述（≤200 码元）。 */
  readonly description: string;
  /** 场景出口钩（≤120 码元，可选）。 */
  readonly exitHook?: string;
}

export interface SceneBeatPlan {
  readonly chapter: number;
  /** 场景节拍 ≤6（解析时 clamp）。 */
  readonly scenes: ReadonlyArray<SceneBeat>;
}

export const SCENE_BEATS_MAX_SCENES = 6;

const clamp = (value: string, max: number): string =>
  value.length > max ? value.slice(0, max) : value;

const stripFence = (content: string): string => {
  const fenced = content.match(/```(?:json)?\s*([\s\S]*?)\s*```/i);
  return (fenced?.[1] ?? content).trim();
};

const firstJsonObject = (content: string): Record<string, unknown> | undefined => {
  const match = /\{[\s\S]*\}/.exec(content);
  if (!match) return undefined;
  try {
    return JSON.parse(match[0]) as Record<string, unknown>;
  } catch {
    return undefined;
  }
};

/**
 * 解析 planner 产出的场景节拍计划：scenes clamp ≤6、字段截断；
 * 无有效 scene（title/description 全空）返回 undefined。
 */
export function parseSceneBeatPlan(content: string, chapter: number): SceneBeatPlan | undefined {
  const parsed = firstJsonObject(stripFence(content));
  const rows = parsed?.scenes;
  if (!Array.isArray(rows)) return undefined;
  const scenes: SceneBeat[] = [];
  rows.forEach((row, index) => {
    if (scenes.length >= SCENE_BEATS_MAX_SCENES) return;
    const record = row as Record<string, unknown>;
    const title = typeof record.title === "string" ? clamp(record.title.trim(), 60) : "";
    const description =
      typeof record.description === "string" ? clamp(record.description.trim(), 200) : "";
    if (!title && !description) return;
    const exitHook =
      typeof record.exitHook === "string" && record.exitHook.trim()
        ? clamp(record.exitHook.trim(), 120)
        : undefined;
    scenes.push({
      id: typeof record.id === "string" && record.id.trim() ? record.id.trim() : `s${index + 1}`,
      title,
      description,
      ...(exitHook ? { exitHook } : {}),
    });
  });
  if (scenes.length === 0) return undefined;
  return { chapter, scenes };
}

/** planner 提示：产出场景节拍 JSON（zh/en 双语）。 */
export function buildSceneBeatsPrompt(params: {
  readonly goal: string;
  readonly outlineNode?: string;
  readonly sceneCount?: number;
  readonly language?: "zh" | "en";
}): string {
  const count = params.sceneCount ?? 3;
  const isEn = params.language === "en";
  if (isEn) {
    return `Break this chapter into ${count} scene beats.

Chapter goal: ${params.goal}${params.outlineNode ? `\nOutline: ${params.outlineNode}` : ""}
Output JSON: {"scenes":[{"id":"s1","title":"...","description":"what happens in this scene (concrete, actionable)","exitHook":"optional line that pushes into the next scene"}]}`;
  }
  return `把本章拆分为 ${count} 个场景节拍。

章节目标：${params.goal}${params.outlineNode ? `\n大纲：${params.outlineNode}` : ""}
输出 JSON：{"scenes":[{"id":"s1","title":"...","description":"本场景发生什么（具体、可执行）","exitHook":"推向下一场景的出口钩（可选）"}]}`;
}

/** writer 注入块：按节拍顺序推进的显式纪律（zh/en 双语）。 */
export function buildSceneBeatsWriterBlock(
  plan: SceneBeatPlan,
  language: "zh" | "en" = "zh",
): string {
  const isEn = language === "en";
  const header = isEn
    ? "## Scene beats (write in this order)"
    : "## 场景节拍（按此顺序推进）";
  const lines = plan.scenes.map((scene) => {
    const parts = [`- ${scene.title}: ${scene.description}`];
    if (scene.exitHook) parts.push(`  ${isEn ? "exit" : "出口"}: ${scene.exitHook}`);
    return parts.join("\n");
  });
  const rule = isEn
    ? "Each beat must land before the next begins; do not skip or merge beats."
    : "每个节拍必须落实后才进入下一拍；不得跳过或合并节拍。";
  return [header, ...lines, rule].join("\n");
}
