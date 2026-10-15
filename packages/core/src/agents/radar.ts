import { BaseAgent } from "./base.js";
import type { Platform, Genre } from "../models/book.js";
import type { RadarSource, PlatformRankings } from "./radar-source.js";
import { FanqieRadarSource, QidianRadarSource } from "./radar-source.js";
import { RadarResultToolSchema } from "./radar-tool.js";

export interface RadarResult {
  readonly recommendations: ReadonlyArray<RadarRecommendation>;
  readonly marketSummary: string;
  readonly timestamp: string;
}

export interface RadarRecommendation {
  readonly platform: Platform;
  readonly genre: Genre;
  readonly concept: string;
  // 707 号：选后再析（G14a/335）恢复——confidence/crowding/differentiation 由
  // analyzeRankings 产出（Rust RadarRecommendation 同水位 serde Value 镜像）；
  // 2.0 harness scan()（submitStructured）不产出 confidence，故可选。
  readonly confidence?: number;
  readonly reasoning: string;
  readonly benchmarkTitles: ReadonlyArray<string>;
  /** G14a/335 号信号卡：该题材当前拥挤度（榜单扎堆程度）。 */
  readonly crowding?: "high" | "medium" | "low";
  /** G14a/335 号信号卡：差异化机会（一句话）。 */
  readonly differentiation?: string;
}

/**
 * G14a/335 号"选后再析"：勾选范围。空数组/缺省 = 该维度不过滤；
 * 全空 = 全量分析（与旧行为一致）。
 */
export interface RadarSelection {
  readonly platforms?: ReadonlyArray<string>;
  readonly categories?: ReadonlyArray<string>;
  readonly titles?: ReadonlyArray<string>;
}

const DEFAULT_SOURCES: ReadonlyArray<RadarSource> = [
  new FanqieRadarSource(),
  new QidianRadarSource(),
];

/** 免费扫榜：只抓各源榜单，不调 LLM。 */
export async function fetchRankings(
  sources: ReadonlyArray<RadarSource> = DEFAULT_SOURCES,
): Promise<ReadonlyArray<PlatformRankings>> {
  return Promise.all(sources.map((s) => s.fetch()));
}

/** 按勾选范围过滤榜单（纯函数）：三维度独立，空 = 不过滤该维度。 */
export function filterRankingsBySelection(
  rankings: ReadonlyArray<PlatformRankings>,
  selection?: RadarSelection,
): ReadonlyArray<PlatformRankings> {
  if (!selection) return rankings;
  const platforms = selection.platforms ?? [];
  const categories = selection.categories ?? [];
  const titles = selection.titles ?? [];
  const platformSet = new Set(platforms.map((p) => p.toLowerCase()));
  const categorySet = new Set(categories.map((x) => x.toLowerCase()));
  const titleSet = new Set(titles.map((x) => x.toLowerCase()));

  return rankings
    .filter((r) => platformSet.size === 0 || platformSet.has(r.platform.toLowerCase()))
    .map((r) => {
      const entries = r.entries.filter((e) => {
        if (categorySet.size > 0 && [...categorySet].some((cat) => (e.category ?? "").toLowerCase().includes(cat))) return true;
        if (titleSet.size > 0 && [...titleSet].some((t) => e.title.toLowerCase().includes(t))) return true;
        return categorySet.size === 0 && titleSet.size === 0;
      });
      return { ...r, entries };
    })
    .filter((r) => r.entries.length > 0);
}

function formatRankingsForPrompt(rankings: ReadonlyArray<PlatformRankings>): string {
  const sections = rankings
    .filter((r) => r.entries.length > 0)
    .map((r) => {
      const lines = r.entries.map(
        (e) => `- ${e.title}${e.author ? ` (${e.author})` : ""}${e.category ? ` [${e.category}]` : ""} ${e.extra}`,
      );
      return `### ${r.platform}\n${lines.join("\n")}`;
    });

  return sections.join("\n\n");
}

export class RadarAgent extends BaseAgent {
  private readonly sources: ReadonlyArray<RadarSource>;

  constructor(
    ctx: ConstructorParameters<typeof BaseAgent>[0],
    sources?: ReadonlyArray<RadarSource>,
  ) {
    super(ctx);
    this.sources = sources ?? DEFAULT_SOURCES;
  }

  get name(): string {
    return "radar";
  }

  /** 免费扫榜（不调 LLM）：榜单数据供前端勾选范围。（707 号恢复，G14a/335） */
  async scanRankings(): Promise<ReadonlyArray<PlatformRankings>> {
    return fetchRankings(this.sources);
  }

  /**
   * 对勾选范围做 LLM 分析（选后再析）：只把范围内的榜单喂给模型，
   * 产出信号卡（拥挤度 crowding + 差异化机会 differentiation）。
   * fork 原实现逐字（engine-rs run_radar_analyze 同水位对偶）。
   */
  async analyzeRankings(
    rankings: ReadonlyArray<PlatformRankings>,
    selection?: RadarSelection,
  ): Promise<RadarResult> {
    const scoped = filterRankingsBySelection(rankings, selection);
    const scopeNote = selection && (selection.platforms?.length || selection.categories?.length || selection.titles?.length)
      ? `（本次分析仅针对用户勾选的范围，请严格基于以下数据）`
      : "";
    // 空 sections 回退语在 analyzeRankings 内联（2.0 scan() 依赖空文本
    // 守卫抛无证据错误，formatRankingsForPrompt 本体不可加回退）。
    const rankingsText = `${scopeNote}\n${formatRankingsForPrompt(scoped) || "（未能获取到实时排行数据，请基于你的知识分析）"}`.trim();

    const systemPrompt = `你是一个专业的网络小说市场分析师。下面是从各平台实时抓取的排行榜数据，请基于这些真实数据分析市场趋势。

## 实时排行榜数据

${rankingsText}

分析维度：
1. 从排行榜数据中识别当前热门题材和标签
2. 分析哪些类型的作品占据榜单高位
3. 发现市场空白和机会点（榜单上缺少但有潜力的方向）
4. 风险提示（榜单上过度扎堆的题材，给出拥挤度）

输出格式必须为 JSON：
{
  "recommendations": [
    {
      "platform": "平台名",
      "genre": "题材类型",
      "concept": "一句话概念描述",
      "confidence": 0.0-1.0,
      "reasoning": "推荐理由（引用具体榜单数据）",
      "benchmarkTitles": ["对标书1", "对标书2"],
      "crowding": "high|medium|low（该题材当前拥挤度）",
      "differentiation": "差异化机会一句话（如何避开扎堆）"
    }
  ],
  "marketSummary": "整体市场概述（基于真实榜单数据）"
}

推荐数量：3-5个，按 confidence 降序排列。`;

    const response = await this.chat(
      [
        { role: "system", content: systemPrompt },
        {
          role: "user",
          content: `请基于上面的实时排行榜数据，分析当前网文市场热度，给出开书建议。`,
        },
      ],
      { temperature: 0.6 },
    );

    return this.parseResult(response.content);
  }

  private parseResult(content: string): RadarResult {
    const jsonMatch = content.match(/\{[\s\S]*\}/);
    if (!jsonMatch) {
      throw new Error("Radar output format error: no JSON found");
    }

    try {
      const parsed = JSON.parse(jsonMatch[0]);
      return {
        recommendations: parsed.recommendations ?? [],
        marketSummary: parsed.marketSummary ?? "",
        timestamp: new Date().toISOString(),
      };
    } catch (e) {
      throw new Error(`Radar JSON parse error: ${e}`);
    }
  }

  async scan(): Promise<RadarResult> {
    const rankings = await Promise.all(this.sources.map((s) => s.fetch()));
    const rankingsText = formatRankingsForPrompt(rankings);
    if (!rankingsText) {
      throw new Error("Market radar has no source evidence to analyze.");
    }

    const systemPrompt = `按已激活的长篇市场研究 Skill 分析以下实时排行榜。每条判断必须引用输入中的具体证据。

## 实时排行榜数据

${rankingsText}

通过结果工具提交建议和整体市场概述。`;

    const { result } = await this.submitStructured(
      [
        { role: "system", content: systemPrompt },
        {
          role: "user",
          content: `请基于上面的实时排行榜数据，分析当前网文市场热度，给出开书建议。`,
        },
      ],
      {
        name: "submit_market_radar",
        label: "Submit market radar",
        description: "Submit evidence-grounded market recommendations in ranked order.",
        parameters: RadarResultToolSchema,
      },
      {  },
    );

    return {
      recommendations: result.recommendations,
      marketSummary: result.marketSummary,
      timestamp: new Date().toISOString(),
    };
  }
}
