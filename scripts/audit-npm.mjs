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
// 501 号：白名单清零——esbuild（vite 7.3.6）、brace-expansion/postcss-selector-
// parser（override 直达补丁线）、fast-xml-parser/provider-utils（override 出清，
// 同 major 补丁线）、vitest/@vitest/mocker（482 号 vitest 5）全部修复。
// 维护规则不变：新增条目必须给出定性理由与来源；移除条目即恢复拦截。
// 688 号：上游 2.0 合并带来依赖树整体换代（约 185 条 → override 直达补丁线
// 消化至 5 条；30 键 override 见根 package.json pnpm.overrides），剩余为
// 跨 major 决策/无补丁线/低危渲染面，逐条定性如下：
const ACCEPTED = new Map([
  // vitest/@vitest/mocker：补丁线在 vitest 4.1.11+，上游 2.0 pin ^3.0.0（版本对齐 2.0.0
  // 是上游发布契约）——跨 major 升级须随上游动作，不在 fork 侧单独跳线。
  // dev-only 暴露面：UI server（GHSA-5xrq）与 mocker redirect（GHSA-82fw）仅在
  // 跑测试时监听/生效，不进生产产物。上游 vitest 升 4 时移除本条即恢复拦截。
  ["vitest", { reason: "上游 2.0 pin ^3.0.0 跨 major 待随上游升级；dev-only（UI server 仅测试期监听）", since: "688" }],
  ["@vitest/mocker", { reason: "同 vitest：补丁线 4.1.11+ 依赖 vitest 4；dev-only（mocker 仅测试期生效）", since: "688" }],
  // braces GHSA-vfj7-8cjw-p6xm：patched <0.0.0（无补丁线，501 号同型先例——
  // 无补丁线即当前最新为终态，跟进上游发版）。
  ["braces", { reason: "无补丁线（patched <0.0.0），当前最新即终态，跟进上游", since: "688" }],
  // katex GHSA-238p-pmpm-9mq7（low）：补丁线 >=0.18.2 为跨 major（streamdown 依赖
  // katex 0.16 线）；fork 阅读器渲染自有内容，原型污染需恶意 LaTeX 输入。
  // streamdown 升级 katex 线时移除本条即恢复拦截。
  ["katex", { reason: "low；补丁线 0.18.2 跨 major（streamdown 依赖 0.16 线）；渲染面需恶意输入方可触发", since: "688" }],
  // 700 号：esbuild GHSA-g7r4-m6w7-qqqr（low，Windows dev server 任意读）——
  // 补丁线 >=0.28.1 但 vite 6.4.1 与 esbuild 0.25 配对；esbuild@<0.28 override
  // 强提 0.28 会破坏 vite:esbuild-transpile（studio client build 活体实证）。
  // 升级随 vite bump（vite 7 线自带新 esbuild），到时移除本条即恢复拦截。
  ["esbuild", { reason: "low；补丁线 >=0.28.1 与 vite 6.4.1 的 0.25 配对冲突（override 强提破坏 vite 构建，700 号活体实证）；dev server 面", since: "700" }],
  // 758 号：deepmerge GHSA-gcqx-w48m-w2h4（low，mergeObject 原型污染）——
  // patched <0.0.0（advisory 明示无修复版，4.3.1 即最新终态，501 号 braces
  // 同型先例）。消费面=streamdown→rehype-sanitize→hast-util-sanitize 渲染链
  // （Markdown 安全过滤配置合并，均为静态配置非用户可控源），跟进上游发版
  // 后移除本条即恢复拦截。
  ["deepmerge", { reason: "low；无补丁线（patched <0.0.0，4.3.1 即最新终态）；streamdown→rehype-sanitize 渲染链静态配置合并面", since: "758" }],
  // 758 号：zod GHSA-mrhx-c7xc-m3w5（high，超大受限集合校验内存耗尽 DoS）——
  // 补丁线 4.6.6（3.x 无补丁线），树内 3.25.76 是 @modelcontextprotocol/sdk /
  // @anthropic-ai/sdk / zod-to-json-schema 的 peer 契约（688 号 vitest 同型：
  // 跨 major 升级须随上游 SDK 动作）。暴露面定性=单机桌面应用，zod 校验输入
  // 来自本地 LLM provider 与用户内容，无远程攻击者可控入口；上游 MCP SDK 升
  // zod 4 契约时移除本条即恢复拦截。
  ["zod", { reason: "high；补丁线 4.6.6 跨 major（3.x 无补丁线），MCP/Anthropic SDK peer 契约待随上游升级；单机桌面无远程攻击面", since: "758" }],
  // 759 号：@hono/node-server GHSA-rmxm-3fg6-px4f（moderate，serveStatic 路径
  // 二次解码致静态前缀中间件绕过；非目录穿越）——漏洞区间 >=1.19.10 <2.1.3，
  // 1.x 线 latest-1（1.19.17，即当前唯一解析线）仍在区间内，补丁仅存在于
  // 2.1.3+（跨 major 须随上游 2.x 动作，688 号 vitest / 758 号 zod 同型）。
  // 暴露面定性=fork 调用图零命中：studio server.ts 仅消费 serve（grep 实证
  // serveStatic 全仓零引用），静态服务为自实现路由；公告仅影响挂中间件的
  // serveStatic 使用方。上游升 2.x 时移除本条即恢复拦截。
  ["@hono/node-server", { reason: "moderate；补丁线 2.1.3 跨 major（1.x 终态 1.19.17 仍在漏洞区间），须随上游升级；fork 零调用面（仅 serve，serveStatic 零引用）", since: "759" }],
  // 784 号：minimatch GHSA-g3hv-75wg-m249（moderate，globparts 放大 DoS）——
  // 漏洞区间 <11.0.1，补丁线 11.0.1 跨 major；仓内两条依赖链均锁旧 major
  //（epub-gen-memory→ejs→jake→filelist 锁 ^5；shadcn→ts-morph→@ts-morph/common
  // 锁 ^10），override 强提 11 破坏传递契约。暴露面定性=运行时零执行：
  // jake/filelist 是 ejs 的构建工具链依赖（引擎只调 ejs render，jake CLI
  // 任务从不执行）；ts-morph 链=shadcn 纯 dev CLI。且 DoS 需攻击者可控
  // glob 模式入参，单机引擎无该输入面。ejs/ts-morph 升级到改用 minimatch 11
  // 的版本时移除本条即恢复拦截。
  ["minimatch", { reason: "moderate；补丁线 11.0.1 跨 major（filelist 锁 ^5、@ts-morph/common 锁 ^10，override 强提破坏传递契约）；运行时零执行（jake/filelist 为 ejs 构建工具链，引擎仅 ejs render；ts-morph 纯 dev）且无攻击者可控 glob 入参面", since: "784" }],
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
