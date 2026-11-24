import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * 1664 号：路由覆盖清单守卫（棘轮）。
 *
 * 1663 号实证：R26 run-log 写入点自 688 号合并起缺失，端点在位却恒空一个
 * 合并周期——掩盖它的正是「断言显式跳过 + check 标签声称覆盖」。同族盲区
 * 的结构性来源：server.ts 的路由没有任何一处测试/探针引用。
 *
 * 规则：server.ts 每条路由，必须在 studio 测试、scripts 探针或 e2e spec 的
 * 源码字面量里出现一次（路径模式转正则匹配，参数段匹配任意单段）。
 * - 新增路由无引用 → 红（新增面必须有可达断言）。
 * - 基线条目获得覆盖 → 红（提示从基线移除；基线只许缩短，不许扩张）。
 * - 基线条目对应路由已删除/改名 → 红（基线陈旧）。
 *
 * 静态字符串匹配是必要条件而非充分条件——有引用不等于断言有效（那由
 * 各测试本体负责）；本守卫只堵「零引用」这一种确定性盲区。
 */
const GUARD_FILE = "route-coverage-guard.test.ts";
const studioSrc = fileURLToPath(new URL("..", import.meta.url));
const repoRoot = resolve(studioSrc, "..", "..", "..");

/**
 * 零引用基线（1664 号首扫：218 条路由 / 184 覆盖 / 34 盲区，本轮清偿两条；
 * 1667 号清偿 28 条：章版本读/恢复 + roster/confirm（book-data-path-family.test）、
 * audit/revise/rewrite（fork-write-faces-llm.test，canonical walkthrough-mock 上游）、
 * 其余 22 条不依赖 LLM 的校验/简单成功路径（route-baseline-batch2.test）。
 * 1669 号清偿 2 条：books/create 与 daemon/start 的「LLM 未配置 → 400
 * LLM_CONFIG_ERROR」负向路径（llm-config-error.test）。
 * 1670 号清偿最后 2 条：文风导入与翻译运行的同一负向路径（llm-config-error.test
 * 的 1670 清单）——**基线清零**，每条 server.ts 路由都至少有一处测试/探针引用。
 * 守卫只认引用，不认断言强度：这些面的成功路径仍多未经 LLM 实跑覆盖。
 *
 * 基线只许缩短：`BASELINE_CEILING` 钉住上限，往基线里加条目即红。
 */
const BASELINE: ReadonlyArray<string> = [];
const BASELINE_CEILING = 0;

function walk(dir: string, out: string[] = []): string[] {
  let names: string[];
  try {
    names = readdirSync(dir);
  } catch {
    return out;
  }
  for (const name of names) {
    if (name === "node_modules" || name === "dist" || name === "target" || name === ".git") continue;
    const full = join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

/** Hono 路由模式 → 匹配源码字面量的正则（路径段边界锚定）。 */
function patternToRegExp(path: string): RegExp {
  let source = "";
  for (let i = 0; i < path.length; i += 1) {
    const char = path[i]!;
    if (char === ":") {
      const brace = path.indexOf("{", i);
      const slash = path.indexOf("/", i);
      const segmentEnd = slash === -1 ? path.length : slash;
      if (brace !== -1 && brace < segmentEnd) {
        const close = path.indexOf("}", brace);
        source += `(?:${path.slice(brace + 1, close)})`;
        i = close;
      } else {
        source += "[^/]+";
        i = segmentEnd - 1;
      }
    } else if (char === "*") {
      source += ".*";
    } else {
      source += char.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    }
  }
  return new RegExp(`(?<![\\w/.-])${source}(?![\\w/-])`);
}

function collectRoutes(): Array<{ verb: string; path: string }> {
  const server = readFileSync(join(studioSrc, "api", "server.ts"), "utf8");
  return [...server.matchAll(/app\.(get|post|put|delete|patch)\(\s*"([^"]+)"/g)]
    .map((match) => ({ verb: match[1]!.toUpperCase(), path: match[2]! }))
    // SPA 兜底 catch-all 不是 API 契约面。
    .filter((route) => route.path !== "*");
}

function collectCorpus(): string[] {
  const files = [
    ...walk(studioSrc).filter((file) => /\.test\.tsx?$/.test(file) && !file.endsWith(GUARD_FILE)),
    ...walk(join(repoRoot, "scripts")).filter((file) => /\.(mjs|js|ts)$/.test(file)),
    ...walk(join(repoRoot, "packages", "studio", "e2e")).filter((file) => /\.(mjs|js|ts)$/.test(file)),
  ];
  return files.map((file) => readFileSync(file, "utf8"));
}

describe("route coverage guard (1664)", () => {
  it("every server route is referenced by a test or probe, except the pinned baseline", () => {
    const routes = collectRoutes();
    expect(routes.length).toBeGreaterThan(200);
    const corpus = collectCorpus();
    expect(corpus.length).toBeGreaterThan(100);

    const uncovered = routes
      .filter((route) => !corpus.some((text) => patternToRegExp(route.path).test(text)))
      .map((route) => `${route.verb} ${route.path}`);

    const routeKeys = new Set(routes.map((route) => `${route.verb} ${route.path}`));
    const stale = BASELINE.filter((entry) => !routeKeys.has(entry));
    expect(stale, "基线陈旧：下列条目对应的路由已不存在，请从 BASELINE 删除").toEqual([]);

    const newlyUncovered = uncovered.filter((entry) => !BASELINE.includes(entry));
    expect(
      newlyUncovered,
      "新增路由没有任何测试/探针引用——请补 studio 测试、scripts 探针或 e2e（不得加入基线）",
    ).toEqual([]);

    const nowCovered = BASELINE.filter((entry) => routeKeys.has(entry) && !uncovered.includes(entry));
    expect(
      nowCovered,
      "下列基线条目已获得测试/探针引用——请从 BASELINE 删除（基线只许缩短）",
    ).toEqual([]);

    // 棘轮单调性：盲区数不得超过基线规模。
    expect(uncovered.length).toBeLessThanOrEqual(BASELINE.length);
    // 基线自身也只许缩短——往里加条目来消音等于把守卫关掉。
    expect(
      BASELINE.length,
      "基线扩张：BASELINE 条目数超过上限（基线只许缩短；新路由必须补引用）",
    ).toBeLessThanOrEqual(BASELINE_CEILING);
    expect(uncovered, "零引用路由").toEqual([]);
  });
});
