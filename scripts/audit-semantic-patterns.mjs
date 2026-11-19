#!/usr/bin/env node
import { readdir, readFile } from "node:fs/promises";
import { join, relative } from "node:path";

const ROOT = process.cwd();
const SCAN_DIRS = [
  "packages/core/src",
  "packages/studio/src",
  "packages/cli/src",
];

const SOURCE_EXTENSIONS = new Set([".ts", ".tsx"]);

const FORBIDDEN_SOURCE_PATHS = [
  "packages/core/src/models/state.ts",
  // 850 号摘除 models/genre-profile.ts 条目：路径与上游 2.0 合并后的合法 zod 流派
  // 档案 schema（GenreProfileSchema，parseGenreProfile）撞名——语义已彻底换代，
  // 保留条目让本工具每次运行必报一条「forbidden remains」狼来了（849 号实测）。
  "packages/core/src/state/memory-db.ts",
  "packages/core/src/interactive-film/memory-link.ts",
  "packages/core/src/agent/llm-stub.ts",
  "packages/cli/src/commands/genre.ts",
];

const FORBIDDEN_ARCHITECTURE_TOKENS = [
  ["audit-failed", "hidden content rejection state"],
  ["review-failed", "hidden content rejection state"],
  ["qualityScore", "host-owned prose score"],
  ["isOutsideHardRange", "host-derived hard length gate"],
  ["isOutsideSoftRange", "host-derived soft length gate"],
  ["lengthWarning", "host-derived length verdict"],
  ["softMin", "host-derived length range"],
  ["hardMin", "host-derived length range"],
  ["SHORT_FICTION_MIN_CHAPTERS", "host-owned creative range"],
  ["SHORT_FICTION_MAX_CHAPTERS", "host-owned creative range"],
  ["INKOS_AGENT_LLM_STUB", "production test-model branch"],
  ["contextRecipes", "unused parallel context abstraction"],
  ["createEditTool", "unregistered generic write surface"],
  ["createWriteFileTool", "unregistered generic write surface"],
  ["plainToAgentMessages", "second conversation-history source"],
  ["InteractionEventSchema", "parallel interaction event ledger"],
  ["AuditIssue", "legacy review issue protocol"],
  ["repairScope", "legacy review issue protocol"],
  ["reviewIssuesAsObservations", "legacy review conversion layer"],
  ["reviewObservations", "legacy review conversion layer"],
  ["reviewIssues", "parallel translation review protocol"],
  ["ValidationWarning", "parallel state-review protocol"],
  ["buildStateReconciliationIssues", "state-review conversion layer"],
  ["loadAllSkillResources", "eager Skill reference loading"],
  ["MAX_SKILL_ACTIVATION_BYTES", "eager Skill reference loading"],
  ["nextActions", "unused speculative ActionResult field"],
  ["fixedIssues", "model-authored revision success claim"],
  ["removeFailedWork", "destructive cleanup of recoverable creation state"],
  ["normalizeStageLabel", "UI stage inferred from human-facing prose"],
  ["stripTrailingLicense", "semantic source text removed by a host heuristic"],
  ["warning.startsWith(\"packaging\")", "observation code inferred from human-facing prose"],
  ["log.startsWith(\"[error]\")", "tool log severity inferred from human-facing prose"],
  ["kind: z.enum([\"hard\", \"soft\"])", "unused host-owned observation severity"],
  ["Type.Literal(\"hard\"), Type.Literal(\"soft\")", "unused host-owned observation severity"],
  ["powerSystem: Type.String({ minLength: 1 })", "genre-specific fanfic canon requirement"],
  ["status.startsWith(\"Error", "UI status inferred from prose"],
  ["content.startsWith(\"\\u2717\")", "UI error state inferred from prose"],
  ["chapterWordCount: z.number().int().min(1000)", "host-owned chapter length floor"],
  ["maximum: 20", "host-owned multi-chapter action ceiling"],
  [".max(20)", "host-owned multi-chapter action ceiling"],
  ["isTerminalProductionToolName", "host-owned production action stop list"],
  ["hasUnansweredTerminalToolResult", "host-owned production action stop list"],
  ["localAssistantStopStream", "host-forced Pi termination after a production action"],
  ["pushToolResultsAsUser", "structured tool results rewritten as user prose"],
  ["appendRestoredHistoryBoundary", "historical control message appended to the user-message stream"],
  ["function suppressManualTextForTool", "tool presentation inferred from an action-name list"],
  ["runWithCliProfileSkills", "legacy CLI-only production wrapper"],
  ["async resyncChapterArtifacts(", "legacy parallel chapter-state repair entry"],
  ["const READ_TOOLS", "capability risk inferred from a parallel tool-name list"],
  ["const DESTRUCTIVE_TOOLS", "capability risk inferred from a parallel tool-name list"],
  ["const CONFIRMED_CREATION_TOOLS", "capability confirmation inferred from a parallel tool-name list"],
  ["function toolRisk(", "capability risk inferred from tool names"],
];

// 1590 号令牌级白名单（854 纪律：白名单按语义定性+file+token 精确钉定）。
// 41 candidates 定性结论：31 条令牌命中全部为 **fork 刻意保留的产品功能面**
// （章节状态契约 audit-failed←187 号 BookTimeline/BookDetail 消费、整书评估
// qualityScore←book-eval 功能核心产物、lengthWarnings←写链长度警告契约、
// repairScope/reviewObservations←修订面契约、.max(20)←模型校验上限）——
// 均有历轮变更记录背书，非上游残留；9 条为启发式（isLikelySemanticDecision
// 的 `.includes(` 业务判定形态）误报，分离为报告面不计红。豁免按「文件精确
// 匹配+令牌子串」钉定——文件级漂移（同文件新增其它令牌命中）仍会红。
const ALLOWED_TOKEN_SITES = {
  "packages/core/src/models/chapter.ts": ["audit-failed", "lengthWarning"],
  "packages/core/src/models/book.ts": [".max(20)"],
  "packages/core/src/models/quality-governance.ts": ["repairScope", ".max(20)", "AuditIssue"],
  "packages/core/src/agents/reviser.ts": ["reviewObservations"],
  "packages/core/src/harness/legacy-book-migration.ts": ["lengthWarning"],
  "packages/core/src/pipeline/runner.ts": ["lengthWarning", "hardMin"],
  "packages/core/src/state/manager.ts": ["lengthWarning"],
  "packages/core/src/utils/analytics.ts": ["qualityScore"],
  "packages/core/src/utils/asset-library.ts": [".max(20)"],
  "packages/core/src/utils/book-eval.ts": ["qualityScore"],
  "packages/studio/src/lib/writing-stats.test.ts": ["audit-failed"],
  "packages/studio/src/pages/BookDetail.tsx": ["audit-failed", "qualityScore"],
  "packages/studio/src/pages/BookTimeline.test.tsx": ["audit-failed"],
  "packages/studio/src/pages/BookTimeline.tsx": ["audit-failed"],
};

const FORBIDDEN_ENTRY_CALLS = [
  "pipeline.initBook(",
  "pipeline.writeNextChapter(",
  "pipeline.writeChapters(",
  "pipeline.reviseDraft(",
  "pipeline.reviseFoundation(",
  "pipeline.resyncChapter",
  "pipeline.importChapters(",
  "pipeline.importCanon(",
  // 1590 号摘除 importFanficCanon（850 先例：清单对现实重核）——fork 同人
  // 导入是数据导入 API（studio 导入按钮活体消费 + agent-tools.ts:1298 已接
  // 工具权威），非 agent 动作链绕道；其余条目零调用=守卫防回归仍有效。
  "pipeline.generateStyleGuide(",
  "runShortFictionProduction(",
  "runScriptCreation(",
  "runStoryboardCreation(",
  "runInteractiveFilmCreation(",
];

const HARNESS_ENTRY_PATHS = [
  "packages/cli/src/commands/",
  "packages/studio/src/api/server.ts",
];

const AGENT_CONSTRUCTION_PATHS = new Set([
  "packages/core/src/agent/agent-session.ts",
  "packages/core/src/agent/worker-agent.ts",
]);

const ACTION_SURFACE_PATHS = [
  "packages/core/src/agent/",
  "packages/core/src/interaction/",
  "packages/studio/src/api/server.ts",
  "packages/studio/src/pages/BookCreate.tsx",
  "packages/cli/src/commands/agent.ts",
  "packages/cli/src/tui/",
];

const SEMANTIC_HINTS = [
  "instruction",
  "intent",
  "requestedIntent",
  "actionSource",
  "free-text",
  "continue",
  "write next",
  "create book",
  "edit",
  "rewrite",
  "revise",
  "play",
  "short",
  "fanfic",
  "spinoff",
  "imitation",
  "续写",
  "继续",
  "建书",
  "短篇",
  "互动",
  "番外",
  "仿写",
  "同人",
];

const PATTERN_TOKENS = [
  ".match(",
  ".test(",
  ".includes(",
  ".startsWith(",
  ".endsWith(",
  "new RegExp(",
];

function hasAny(text, words) {
  const lower = text.toLowerCase();
  return words.some((word) => lower.includes(word.toLowerCase()));
}

function isActionSurface(path) {
  const rel = path.replace(`${ROOT}/`, "");
  return ACTION_SURFACE_PATHS.some((prefix) => rel.startsWith(prefix));
}

function isLikelySemanticDecision(path, line, windowText) {
  if (!isActionSurface(path)) return false;
  if (!PATTERN_TOKENS.some((token) => line.includes(token))) return false;
  if (!hasAny(`${line}\n${windowText}`, SEMANTIC_HINTS)) return false;
  if (line.includes("CHAT_EDIT_TEXT_EXTENSIONS")) return false;
  if (line.includes("SAFE_ROLE_TRUTH_FILE_RE")) return false;
  if (path.endsWith("agent-tools.ts") && line.includes("current.includes(")) return false;
  if (path.endsWith("skill-tool.ts") && line.includes("body.includes(\"\\0\")")) return false;
  if (line.includes("RUNTIME_DIAGNOSTIC_FILE_RE")) return false;
  if (line.includes("runtimeDiagnostic")) return false;
  if (line.includes("startsWith(\"scene-turn-\")")) return false;
  if (line.includes("PlannerParseError") && line.includes(".test(text)")) return false;
  if (line.includes("CODE_FENCE_RE") || line.includes("DIRECTIVE_CLOSE_RE")) return false;
  if (line.includes("safeSessionId")) return false;
  if (line.includes("genreId")) return false;
  if (line.includes("relPath") || line.includes("targetPath") || line.includes("requestedPath")) return false;
  if (line.includes("entry.name") || line.includes("base.includes") || line.includes("escapeRegExp")) return false;
  if (line.includes("raw.includes(\"/\")") || line.includes("raw.includes(\"\\\\\")")) return false;
  if (line.includes("Manual text edit requires review")) return false;
  if (path.endsWith("project-tools.ts") && line.includes("includes(")) return false;
  if (path.endsWith("i18n.ts")) return false;
  if (line.includes("rel || rel.startsWith")) return false;
  if (line.includes("trimmed.match(/^([A-Za-z_]")) return false;
  if (line.includes("file.includes(\"/\")")) return false;
  if (line.includes("startsWith(\"[Tool results]\")")) return false;
  if (line.includes("already processing") || line.includes("prompt.*queue")) return false;
  if (line.includes("trimmed.startsWith(\"#\")")) return false;
  if (line.includes("actionSource") && line.includes("startsWith(\"/\")")) return false;
  if (line.includes("startsWith(\"/\")")) return false;
  // Explicit slash-command grammar and Pi tool namespaces are protocols, not
  // natural-language intent inference.
  if (path.endsWith("agent-input.ts") && line.includes("/^\\/")) return false;
  if ((path.endsWith("agent-session.ts") || path.endsWith("session-transcript-restore.ts"))
    && line.includes("includes(\"__\")")) return false;
  if (line.includes("endsWith(") && !hasAny(windowText, ["instruction", "intent"])) return false;
  return true;
}

async function walk(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "dist" || entry.name === "__tests__") continue;
      out.push(...await walk(path));
    } else if (SOURCE_EXTENSIONS.has(path.slice(path.lastIndexOf(".")))) {
      out.push(path);
    }
  }
  return out;
}

const files = [];
for (const dir of SCAN_DIRS) {
  files.push(...await walk(join(ROOT, dir)));
}

// 848 号零扫描面卫兵：本仓三个包合计 700+ 源文件——若目录结构漂移（包改名/
// ROOT 取错）扫到 0 文件，findings=0 会被误读为「全绿」而非「没扫到」
//（846 号 audit:rust 恒绿事故同型：解析面失效默认绿）。
if (files.length === 0) {
  console.error(`Semantic audit: scan surface is EMPTY under ${ROOT} (${SCAN_DIRS.join(", ")}) — directory drift? Not a pass.`);
  process.exit(2);
}

const findings = [];
// 1590 号：启发式「语义决策候选」分离为报告面（不计 exit）——设计意图是
// 待人工审的候选清单，与禁令命中（exit 1 面）混在同一名单曾使本审计
// candidates 41 常态红（850 号时点已红）=狼来了（846/847 纪律）。
const candidates = [];
const allowedCount = { tokenSites: 0 };
for (const path of FORBIDDEN_SOURCE_PATHS) {
  try {
    await readFile(join(ROOT, path), "utf-8");
    findings.push({ file: path, line: 1, text: "forbidden 1.x source remains" });
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}
for (const file of files) {
  const content = await readFile(file, "utf-8");
  const lines = content.split(/\r?\n/);
  const relativeFile = relative(ROOT, file);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const windowText = lines.slice(Math.max(0, index - 4), Math.min(lines.length, index + 5)).join("\n");
    for (const [token, reason] of FORBIDDEN_ARCHITECTURE_TOKENS) {
      if (line.includes(token)) {
        const allowed = (ALLOWED_TOKEN_SITES[relativeFile] ?? []).includes(token);
        if (allowed) {
          allowedCount.tokenSites += 1;
          continue;
        }
        findings.push({
          file: relative(ROOT, file),
          line: index + 1,
          text: `${reason}: ${line.trim()}`,
        });
      }
    }
    if (isLikelySemanticDecision(file, line, windowText)) {
      candidates.push({
        file: relative(ROOT, file),
        line: index + 1,
        text: line.trim(),
      });
    }
    if (
      HARNESS_ENTRY_PATHS.some((prefix) => relativeFile.startsWith(prefix))
      && FORBIDDEN_ENTRY_CALLS.some((token) => line.includes(token))
    ) {
      findings.push({
        file: relativeFile,
        line: index + 1,
        text: `production entry bypasses Harness capability: ${line.trim()}`,
      });
    }
    if (line.includes("new Agent(") && !AGENT_CONSTRUCTION_PATHS.has(relativeFile)) {
      findings.push({
        file: relativeFile,
        line: index + 1,
        text: `parallel Agent construction outside the shared main/worker kernels: ${line.trim()}`,
      });
    }
  }
}

if (process.argv.includes("--json")) {
  console.log(JSON.stringify({ findings, candidates, allowedCount }, null, 2));
} else {
  console.log(`Semantic/template-pattern audit findings: ${findings.length}`);
  for (const finding of findings) {
    console.log(`${finding.file}:${finding.line}  ${finding.text}`);
  }
  // 候选为报告面（1589 号分级）：启发式语义决策形态待人工审，不计 exit。
  console.log(`Semantic heuristic candidates (report-only): ${candidates.length}`);
  for (const candidate of candidates) {
    console.log(`  [candidate] ${candidate.file}:${candidate.line}  ${candidate.text}`);
  }
  if (allowedCount.tokenSites > 0) {
    console.log(`Allowed token sites (whitelisted, 1590): ${allowedCount.tokenSites}`);
  }
}

if (findings.length > 0) process.exitCode = 1;
