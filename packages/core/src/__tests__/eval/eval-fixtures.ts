//! R8 行为评测集（369 号，二轮 P2）——共享 fixtures 与构造 helper。
//!
//! 防提示词漂移：断言各 agent 的 system/user prompt 锚点节（维度表、输出
//! 格式模板、纪律规则）不随重构丢失；回放：mock LLM 固定产物 → 断言解析
//! 器结构化产出。`pnpm eval`（根目录）= `vitest run src/__tests__/eval`。
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const ZERO_USAGE = { promptTokens: 0, completionTokens: 0, totalTokens: 0 };

/** 样章文本（约 300 字，含钩子动静与对话，供 continuity/settler 评测）。 */
export const SAMPLE_CHAPTER = [
  "林动在废墟边缘停下脚步，掌心的祖符石微微发烫。",
  "「再往前一步，就出了青镇的边界。」老陈的声音从身后传来，压得很低。",
  "他没有回头。三年前的那个雨夜，父亲就是从这里消失的。怀表在口袋里静止不动——只要踏过这条线，它就会重新走动。",
  "远处传来巡逻队的犬吠。林动深吸一口气，把祖符石攥进手心，迈出了那一步。",
  "怀表的指针，动了。",
].join("\n");

/** settler 输出 fixture（delta JSON + TENSION_METRICS 双节完整形态）。 */
export const SAMPLE_SETTLEMENT_OUTPUT = [
  "=== POST_SETTLEMENT ===",
  "本章状态变动：林动越界；伏笔推进：父亲失踪线。",
  "",
  "=== RUNTIME_STATE_DELTA ===",
  "```json",
  "{",
  '  "chapter": 12,',
  '  "currentStatePatch": { "currentLocation": "青镇边界外" },',
  '  "hookOps": { "upsert": [], "mention": [], "resolve": [], "defer": [] },',
  '  "newHookCandidates": [],',
  '  "chapterSummary": {',
  '    "chapter": 12,',
  '    "title": "越界",',
  '    "characters": "林动,老陈",',
  '    "events": "林动踏出青镇边界，怀表复走",',
  '    "stateChanges": "位置变更",',
  '    "hookActivity": "mentor-debt advanced",',
  '    "mood": "紧绷",',
  '    "chapterType": "推进章"',
  "  },",
  '  "subplotOps": [],',
  '  "emotionalArcOps": [],',
  '  "characterMatrixOps": [],',
  '  "notes": []',
  "}",
  "```",
  "",
  "=== TENSION_METRICS ===",
  "conflictLevel: 7",
  "revealLevel: 6",
].join("\n");

/** continuity 审查响应 fixture（含 critical + repair_scope 路由）。 */
export const SAMPLE_AUDIT_RESPONSE = JSON.stringify({
  passed: false,
  overall_score: 62,
  issues: [
    {
      severity: "critical",
      repair_scope: "structural",
      category: "timeline-coherence",
      description: "怀表时间线与第 3 章设定冲突",
      suggestion: "补充怀表停摆的触发事件",
    },
    {
      severity: "info",
      repair_scope: "local",
      category: "prose-surface",
      description: "部分段落节奏偏慢",
      suggestion: "可交给 Polisher",
    },
  ],
  summary: "主线推进成立，但时间线存在硬伤。",
});

/** 构造最小 AgentCtx（provider 配置 + 测试模型 + 项目根）。 */
export function makeAgentCtx(projectRoot: string): {
  client: {
    provider: string;
    apiFormat: string;
    stream: boolean;
    defaults: {
      temperature: number;
      maxTokens: number;
      thinkingBudget: number;
      extra: Record<string, never>;
    };
  };
  model: string;
  projectRoot: string;
} {
  return {
    client: {
      provider: "openai",
      apiFormat: "chat",
      stream: false,
      defaults: { temperature: 0.3, maxTokens: 4096, thinkingBudget: 0, extra: {} },
    },
    model: "eval-model",
    projectRoot,
  };
}

/** 建临时书目录（book.json + prompt 覆盖 + story/ 骨架），返回 bookDir/root。 */
export async function makeBookFixture(id = "eval-book", language: "zh" | "en" = "zh"): Promise<{
  root: string;
  bookDir: string;
}> {
  const root = await mkdtemp(join(tmpdir(), `inkos-eval-${id}-`));
  const bookDir = join(root, "book");
  await mkdir(join(bookDir, "story"), { recursive: true });
  await mkdir(join(root, "prompt", "longform"), { recursive: true });
  await writeFile(
    join(bookDir, "book.json"),
    JSON.stringify(
      {
        id,
        title: "评测之书",
        genre: "xuanhuan",
        platform: "qidian",
        chapterWordCount: 800,
        targetChapters: 60,
        status: "active",
        language,
        createdAt: "2026-09-13T00:00:00.000Z",
        updatedAt: "2026-09-13T00:00:00.000Z",
      },
      null,
      2,
    ),
    "utf-8",
  );
  return { root, bookDir };
}
