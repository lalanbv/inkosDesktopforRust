#!/usr/bin/env node
// npm 依赖漏洞审计（467 号；454 号备案脚本化）。
//
// 对 pnpm-lock.yaml 跑 `pnpm audit --json`（npmjs registry——npmmirror 无
// audit 端点，必须显式指定）。语义对齐 scripts/audit-rust.mjs：
// - **白名单放行**：已定性接受风险的 dev 工具链告警只汇总不拦截
//   （每条须注来源循环号与理由；无补丁线的 modulo 3.x 最新即为终态）；
// - **新告警拦截**：白名单之外的任何 advisory 非零退出——新漏洞不应静默绿灯。
//
//   node scripts/audit-npm.mjs            # 全量扫描
//   node scripts/audit-npm.mjs --json     # 附带输出原始 advisory 摘要
//
// 前置：pnpm（同仓库包管理器）。

import { execFileSync } from "node:child_process";

const REGISTRY = "https://registry.npmjs.org/";

/**
 * 已接受风险白名单：module → { reason, since }。
 * 维护规则：新增条目必须给出定性理由与来源；移除条目即恢复拦截。
 */
const ACCEPTED = new Map([
  // esbuild（491 号 vite 7.3.6+studio vite ^7.3.6 出清）、brace-expansion/
  // postcss-selector-parser（491 号 override 直达补丁线）均已移出白名单；
  // vitest/@vitest/mocker 已于 482 号修复。若回归会重新被拦截。
  ["fast-xml-parser", { reason: "pi-ai 0.67.1 精确钉（434 号安全先例）→aws-sdk 链，补丁需跨 major 5.x 且 Bedrock 默认未启用", since: "467 号" }],
  ["@ai-sdk/provider-utils", { reason: "ai@6.0.x 精确钉 provider-utils 4.0.23（补丁 >=4.0.33 需 ai 7 major）——上游阻塞", since: "467 号" }],
]);

const args = process.argv.slice(2);
const verbose = args.includes("--json");

let raw;
try {
  raw = execFileSync("pnpm", ["audit", "--json", `--registry=${REGISTRY}`], {
    encoding: "utf-8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 64 * 1024 * 1024,
  });
} catch (error) {
  const stdout = typeof error.stdout === "string" ? error.stdout : "";
  if (stdout.trim().startsWith("{")) {
    raw = stdout; // pnpm audit 发现漏洞时以非零码退出，stdout 仍是 JSON
  } else {
    console.error("[audit-npm] pnpm audit 执行失败（registry 不可达？）");
    console.error(error.stderr ?? error.message);
    process.exit(3);
  }
}

let parsed;
try {
  parsed = JSON.parse(raw);
} catch {
  console.error("[audit-npm] audit 输出非 JSON");
  process.exit(3);
}

const advisories = Object.values(parsed.advisories ?? {});
if (advisories.length === 0) {
  console.log("[audit-npm] ✓ 0 advisory");
  process.exit(0);
}

const accepted = [];
const unaccepted = [];
for (const a of advisories) {
  const name = a.module_name ?? "?";
  const severity = a.severity ?? "?";
  if (ACCEPTED.has(name)) {
    accepted.push(`  = ${name} [${severity}] ${ACCEPTED.get(name).reason}（${ACCEPTED.get(name).since} 接受）`);
  } else {
    unaccepted.push(`  ✗ ${name} [${severity}] ${a.title ?? ""} ${a.url ?? ""}`.trim());
  }
}

if (verbose) {
  for (const line of [...accepted, ...unaccepted]) console.log(line);
}

console.log(`[audit-npm] advisory 合计 ${advisories.length}：白名单放行 ${accepted.length}，拦截 ${unaccepted.length}`);
if (unaccepted.length > 0) {
  console.error("[audit-npm] ✗ 新增未白名单告警：\n" + unaccepted.join("\n"));
  process.exit(1);
}
console.log("[audit-npm] ✓ 全部为已接受风险（dev 工具链），通过");
