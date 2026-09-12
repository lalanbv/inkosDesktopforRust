import { BaseAgent } from "./base.js";
import {
  buildDirectionCandidatesPrompt,
  parseDirectionCandidates,
  type DirectionCandidate,
  type InspirationCard,
} from "../models/director.js";

/**
 * 导演 Agent（G6/354 号）：灵感卡 → 方向候选批量生成的 LLM 调用链。
 * 候选解析与 prompt 构建均为 352 号契约纯函数；本类只负责 chat 编排。
 */
export class DirectorAgent extends BaseAgent {
  get name(): string {
    return "director";
  }

  async generateDirections(params: {
    readonly inspiration: InspirationCard;
    readonly count?: number;
    readonly excludeTitles?: ReadonlyArray<string>;
    readonly language?: "zh" | "en";
  }): Promise<DirectionCandidate[]> {
    const count = params.count ?? 3;
    const language = params.language ?? "zh";
    const prompt = buildDirectionCandidatesPrompt({
      inspiration: params.inspiration,
      count,
      excludeTitles: params.excludeTitles,
      language,
    });
    const system =
      language === "en" ? "You are the story director." : "你是故事导演。";
    const response = await this.chat(
      [
        { role: "system", content: system },
        { role: "user", content: prompt },
      ],
      { temperature: 0.8 },
    );
    return parseDirectionCandidates(response.content, params.excludeTitles ?? []);
  }
}
