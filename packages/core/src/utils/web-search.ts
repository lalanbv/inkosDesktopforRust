/**
 * Web search + URL fetch utilities.
 *
 * searchWeb(): Tavily API search (requires TAVILY_API_KEY env var).
 * fetchUrl(): Fetch a specific URL and return plain text (SSRF egress guard, see ./egress-guard.js).
 */
import { assertPublicEgressHost } from "./egress-guard.js";

export interface SearchResult {
  readonly title: string;
  readonly url: string;
  readonly snippet: string;
}

export interface WebSearchOptions {
  readonly apiKey?: string;
  readonly apiKeyEnv?: string;
  readonly baseUrl?: string;
}

/**
 * Search the web via Tavily API.
 * Requires TAVILY_API_KEY environment variable.
 * Throws if key is not set — caller should catch and fall back to regular chat.
 */
export async function searchWeb(
  query: string,
  maxResults = 5,
  options: WebSearchOptions = {},
): Promise<ReadonlyArray<SearchResult>> {
  const apiKey = options.apiKey
    || (options.apiKeyEnv ? process.env[options.apiKeyEnv] : undefined)
    || process.env.TAVILY_API_KEY;
  if (!apiKey) {
    throw new Error(`${options.apiKeyEnv ?? "TAVILY_API_KEY"} not set. Configure Studio research search or set the env var to enable web search.`);
  }

  const endpoint = options.baseUrl?.trim() || "https://api.tavily.com/search";
  const res = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Authorization": `Bearer ${apiKey}`,
    },
    body: JSON.stringify({
      api_key: apiKey,
      query,
      max_results: maxResults,
      search_depth: "basic",
    }),
    signal: AbortSignal.timeout(15000),
  });

  if (!res.ok) {
    throw new Error(`Tavily search failed: ${res.status} ${await res.text().catch(() => "")}`);
  }

  const data = await res.json() as { results?: Array<{ title?: string; url?: string; content?: string }> };
  return (data.results ?? []).map((r) => ({
    title: r.title ?? "",
    url: r.url ?? "",
    snippet: r.content ?? "",
  }));
}

/**
 * Fetch a URL and return its complete text content.
 * HTML is stripped to plain text. Context budgeting happens at the consumer.
 *
 * 689 号：SSRF 初判防线重施（556/635 号；防线本体在 ./egress-guard.js，与
 * engine-rs web_search.rs 同水位对偶）。allowPrivateEgress = 用户显式配置豁免
 *（researchSearch.allowPrivateEgress 项目配置透传，自建网关/内网知识库部署
 * 语义，与 Rust fetch_url 第三参同构）；缺省 false 走防线。
 */
export async function fetchUrl(
  url: string,
  options: { allowPrivateEgress?: boolean } = {},
): Promise<string> {
  if (!options.allowPrivateEgress) {
    try {
      await assertPublicEgressHost(url);
    } catch (e) {
      throw new Error(`Fetch blocked: ${(e as Error).message}`);
    }
  }
  const res = await fetch(url, {
    headers: {
      "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
      "Accept": "text/html, application/json, text/plain",
    },
    signal: AbortSignal.timeout(15000),
  });

  if (!res.ok) {
    throw new Error(`Fetch failed: ${res.status} ${res.statusText}`);
  }

  const contentType = res.headers.get("content-type") ?? "";
  const text = await res.text();

  if (contentType.includes("html")) {
    return text
      .replace(/<script[\s\S]*?<\/script>/gi, "")
      .replace(/<style[\s\S]*?<\/style>/gi, "")
      .replace(/<[^>]*>/g, " ")
      .replace(/\s+/g, " ")
      .trim();
  }

  return text;
}
