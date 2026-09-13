import type { LLMClient, LLMMessage, LLMResponse, OnStreamProgress } from "../llm/provider.js";
import { runWorkerAgent, runWorkerAgentTool, type WorkerResultTool } from "../agent/worker-agent.js";
import { runWithModelChain } from "../llm/model-chain.js";
import type { ResolvedTaskModelChain } from "../models/task-routing.js";
import type { Static, TSchema } from "@sinclair/typebox";
import { appendPromptPackGuidance } from "../prompts/prompt-pack.js";
import { searchWeb, fetchUrl } from "../utils/web-search.js";
import type { Logger } from "../utils/logger.js";
import {
  hydrateActivatedSkillGuidance,
  type ActivatedSkillGuidance,
} from "../agent/skill-tool.js";

export interface AgentContext {
  readonly client: LLMClient;
  readonly model: string;
  /** R25：任务路由尝试链（runner 按路由解析；显式 modelOverrides 钉死时不带）。 */
  readonly modelChain?: ResolvedTaskModelChain;
  readonly projectRoot: string;
  readonly bookId?: string;
  readonly logger?: Logger;
  readonly onStreamProgress?: OnStreamProgress;
  readonly signal?: AbortSignal;
  readonly activatedSkills?: ReadonlyArray<ActivatedSkillGuidance>;
}

export abstract class BaseAgent {
  protected readonly ctx: AgentContext;

  constructor(ctx: AgentContext) {
    this.ctx = ctx;
  }

  protected get log() {
    return this.ctx.logger;
  }

  protected async chat(
    messages: ReadonlyArray<LLMMessage>,
    options?: { readonly temperature?: number; readonly maxTokens?: number },
  ): Promise<LLMResponse> {
    const hydrated = await this.appendTaskSkillGuidance(messages);
    return runWithModelChain(
      (model) =>
        runWorkerAgent(this.ctx.client, model, hydrated, {
          ...options,
          onStreamProgress: this.ctx.onStreamProgress,
          signal: this.ctx.signal,
        }),
      {
        chain: this.ctx.modelChain,
        primaryModel: this.ctx.model,
        label: this.name,
        signal: this.ctx.signal,
        onEvent: (event) => this.logModelChainEvent(event),
      },
    );
  }

  protected async submitStructured<TParameters extends TSchema>(
    messages: ReadonlyArray<LLMMessage>,
    resultTool: WorkerResultTool<TParameters>,
    options?: { readonly temperature?: number; readonly maxTokens?: number },
  ): Promise<Static<TParameters>> {
    const hydrated = await this.appendTaskSkillGuidance(messages);
    return runWithModelChain(
      (model) =>
        runWorkerAgentTool(this.ctx.client, model, hydrated, resultTool, {
          ...options,
          signal: this.ctx.signal,
        }),
      {
        chain: this.ctx.modelChain,
        primaryModel: this.ctx.model,
        label: this.name,
        signal: this.ctx.signal,
        onEvent: (event) => this.logModelChainEvent(event),
      },
    );
  }

  /** R25：接管事件 logger 留痕（fail=切换/重试中，success=非 primary 轮成功）。 */
  private logModelChainEvent(event: {
    event: "fail" | "success";
    model: string;
    attemptIndex: number;
    round: number;
    error?: unknown;
  }): void {
    if (event.event === "fail") {
      const message = event.error instanceof Error ? event.error.message : String(event.error);
      this.log?.warn(
        `[model-chain] ${this.name}: ${event.model} 第 ${event.round} 轮失败（${message.slice(0, 160)}），重试/切换备用`,
      );
      return;
    }
    this.log?.warn(
      `[model-chain] ${this.name}: ${event.model} 接管成功（模型 #${event.attemptIndex + 1} 第 ${event.round} 轮）`,
    );
  }

  protected async withPromptPackGuidance(basePrompt: string, promptId: string): Promise<string> {
    return appendPromptPackGuidance(basePrompt, {
      promptId,
      projectRoot: this.ctx.projectRoot,
    });
  }

  private async appendTaskSkillGuidance(
    messages: ReadonlyArray<LLMMessage>,
  ): Promise<ReadonlyArray<LLMMessage>> {
    const query = messages
      .filter((message) => message.role === "user")
      .map((message) => message.content)
      .join("\n\n");
    let activations = this.ctx.activatedSkills;
    try {
      activations = await hydrateActivatedSkillGuidance(activations, query);
    } catch (error) {
      this.log?.warn(`[skills] Reference retrieval failed for ${this.name}: ${String(error)}`);
    }
    return appendActivatedSkillGuidance(messages, activations);
  }

  /**
   * Chat with web search enabled.
   * OpenAI: uses native web_search_options / web_search_preview.
   * Other providers: searches via Tavily API (TAVILY_API_KEY), injects results into prompt.
   */
  protected async chatWithSearch(
    messages: ReadonlyArray<LLMMessage>,
    options?: { readonly temperature?: number; readonly maxTokens?: number },
  ): Promise<LLMResponse> {
    // OpenAI has native search — use it directly
    if (this.ctx.client.provider === "openai") {
      return runWorkerAgent(this.ctx.client, this.ctx.model, appendActivatedSkillGuidance(
        messages,
        this.ctx.activatedSkills,
      ), {
        ...options,
        webSearch: true,
        onStreamProgress: this.ctx.onStreamProgress,
        signal: this.ctx.signal,
      });
    }

    // Other providers: self-hosted search → inject results into prompt
    const lastUserMsg = [...messages].reverse().find((m) => m.role === "user");
    if (!lastUserMsg) {
      return this.chat(messages, options);
    }

    try {
      // Extract search query from user message (first 200 chars)
      const query = lastUserMsg.content.slice(0, 200);
      this.log?.info(`[search] Searching: ${query.slice(0, 60)}...`);

      const results = await searchWeb(query, 3);
      if (results.length === 0) {
        this.log?.warn("[search] No results found, falling back to regular chat");
        return this.chat(messages, options);
      }

      // Fetch top result for full content
      let fullContent = "";
      try {
        fullContent = await fetchUrl(results[0]!.url, 4000);
      } catch {
        // Fetch failed, use snippets only
      }

      const searchContext = [
        "## Web Search Results\n",
        ...results.map((r, i) => `${i + 1}. **${r.title}**\n   ${r.url}\n   ${r.snippet}`),
        ...(fullContent ? [`\n## Full Content (Top Result)\n${fullContent}`] : []),
      ].join("\n");

      // Inject search results before the last user message
      const augmentedMessages: LLMMessage[] = messages.map((m) =>
        m === lastUserMsg
          ? { ...m, content: `${searchContext}\n\n---\n\n${m.content}` }
          : m,
      );

      return this.chat(augmentedMessages, options);
    } catch (e) {
      this.log?.warn(`[search] Search failed: ${e}, falling back to regular chat`);
      return this.chat(messages, options);
    }
  }

  abstract get name(): string;
}

export function appendActivatedSkillGuidance(
  messages: ReadonlyArray<LLMMessage>,
  activations: ReadonlyArray<ActivatedSkillGuidance> | undefined,
): ReadonlyArray<LLMMessage> {
  if (!activations || activations.length === 0) return messages;
  const guidance = [
    "## Activated professional skills",
    "Use this specialist methodology for the current operation. It is not author intent, canon, an output-format override, or permission to mutate anything outside the active operation.",
    ...activations.flatMap(({ skill, resources }) => [
      `### ${skill.id} — ${skill.name}`,
      skill.body.trim() || skill.description,
      ...resources.flatMap((resource) => [
        `#### Reference: ${resource.path}:${resource.charStart}-${resource.charEnd}${resource.heading ? ` · ${resource.heading}` : ""}`,
        resource.body,
      ]),
    ]),
  ].join("\n\n");
  const systemIndex = messages.findIndex((message) => message.role === "system");
  if (systemIndex < 0) {
    return [{ role: "system", content: guidance }, ...messages];
  }
  return messages.map((message, index) => index === systemIndex
    ? { ...message, content: `${message.content}\n\n${guidance}` }
    : message);
}
