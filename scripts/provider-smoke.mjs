#!/usr/bin/env node
// 真 provider 冒烟（630 号入库，配套《对标调研深化五_上游pi1.0迁移施工图》）：
//   node scripts/provider-smoke.mjs --service <name> --model <id> [--api-format chat|responses]
//                                   [--base-url <url>] [--max-tokens 16] [--skip-stream]
//                                   [--project-root <path>] [--timeout-ms 60000]
//
// 背景：上游 pi 1.0 迁移（openai SDK 6→7 传递跳升 + anthropic SDK 凭据链变化）的
// 风险面在 mock 门禁盲区——请求构造/SSE 逐帧解析/错误归一化只有打真实端点才能验证。
// 本脚本就是施工图七步施工序第 6 步的落地工具：key 可得时一条命令完成真 provider
// 往返，无 key 时给出可行动指引并退出码 2。
//
// 每次运行覆盖两条链（均走 core createLLMClient → pi-ai 三适配器之一）：
//   A 非流式：请求构造 + 响应解析（content 非空 + usage 可读）
//   B 流式：SSE 逐帧（onTextDelta ≥1 帧且拼接非空；--skip-stream 可跳过）
// 任一链失败退出码 1；key 缺失退出码 2；参数错误退出码 3。绝不打印 key。
//
// 三适配器覆盖矩阵（施工图验收面）：
//   anthropic-messages → --service anthropic --model claude-sonnet-4-5
//   openai-completions → --service deepseek --model deepseek-chat（或 zai/kimi 等）
//   openai-responses   → --service openai   --model gpt-5

import { existsSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");
const coreDist = join(repoRoot, "packages", "core", "dist", "index.js");

if (!existsSync(coreDist)) {
  console.error(`[provider-smoke] ✗ 找不到 core 构建产物：${coreDist}`);
  console.error("  先跑 `pnpm --filter @actalk/inkos-core build`（或仓根 `pnpm build`）再冒烟。");
  process.exit(3);
}

const { createLLMClient, chatCompletion, getServiceApiKey, resolveServicePreset } = await import(
  `file://${coreDist}`
);

// --- args ---
const args = process.argv.slice(2);
const argOf = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const has = (name) => args.includes(name);

const service = argOf("--service");
const model = argOf("--model");
if (!service || !model) {
  console.error("[provider-smoke] ✗ 用法：node scripts/provider-smoke.mjs --service <name> --model <id> [--api-format chat|responses] [--base-url <url>] [--max-tokens 16] [--skip-stream] [--project-root <path>] [--timeout-ms 60000]");
  process.exit(3);
}
const projectRoot = resolve(argOf("--project-root", process.cwd()));
const maxTokens = Number(argOf("--max-tokens", "16"));
const timeoutMs = Number(argOf("--timeout-ms", "60000"));
const skipStream = has("--skip-stream");
const baseUrlArg = argOf("--base-url");
const preset = resolveServicePreset(service.startsWith("custom:") ? "custom" : service);
const apiFormat = argOf("--api-format", preset?.api === "openai-responses" ? "responses" : "chat");

// --- key 解析（secrets.json → env {SERVICE}_API_KEY），绝不打印值 ---
const apiKey = (await getServiceApiKey(projectRoot, service)) ?? "";
if (!apiKey) {
  console.error(`[provider-smoke] ✗ 未找到 service="${service}" 的 API key（退出码 2）。`);
  console.error("  可行注入方式（任一）：");
  console.error(`    1) ${join(projectRoot, ".inkos", "secrets.json")} → {"services":{"${service}":{"apiKey":"..."}}}`);
  console.error(`    2) 环境变量 ${service.replace(/[^a-zA-Z0-9]/g, "_").toUpperCase()}_API_KEY`);
  console.error("  施工图语境：这是 pi 1.0 迁移第 6 步「真 provider 冒烟」的唯一前提，");
  console.error("  key 就位后重跑本脚本即可开跑迁移验收。");
  process.exit(2);
}

// --- 公共配置 ---
const providerFamily = preset?.providerFamily === "anthropic" ? "anthropic" : "openai";
const baseConfig = {
  service,
  model,
  apiKey,
  apiFormat,
  ...(baseUrlArg ? { baseUrl: baseUrlArg } : {}),
  ...(providerFamily === "anthropic" ? { provider: "anthropic" } : {}),
  temperature: 0.7,
};
const messages = [{ role: "user", content: "Reply with exactly one word: PONG" }];
const callOpts = {
  maxTokens,
  retry: false, // 冒烟要快速 pass-or-fail，跳过 502/503/429 重试退避（doctor 探针先例）
  signal: AbortSignal.timeout(timeoutMs),
};

const results = [];
let failed = false;

// A：非流式
{
  const client = createLLMClient({ ...baseConfig, stream: false });
  const t0 = performance.now();
  try {
    const res = await chatCompletion(client, model, messages, callOpts);
    const ms = Math.round(performance.now() - t0);
    const ok = typeof res.content === "string" && res.content.trim().length > 0;
    results.push({
      chain: "A 非流式", ok, ms,
      usage: res.usage ? `${res.usage.promptTokens}+${res.usage.completionTokens}=${res.usage.totalTokens} tok` : "n/a",
      preview: res.content?.slice(0, 80) ?? "",
    });
    if (!ok) failed = true;
  } catch (err) {
    results.push({ chain: "A 非流式", ok: false, ms: Math.round(performance.now() - t0), usage: "n/a", preview: String(err?.message ?? err).slice(0, 200) });
    failed = true;
  }
}

// B：流式（SSE 逐帧——SDK 7 头号风险面）
if (!skipStream) {
  const client = createLLMClient({ ...baseConfig, stream: true });
  const t0 = performance.now();
  let deltas = 0;
  let streamed = "";
  try {
    const res = await chatCompletion(client, model, messages, {
      ...callOpts,
      onTextDelta: (text) => {
        deltas += 1;
        streamed += text;
      },
    });
    const ms = Math.round(performance.now() - t0);
    const ok = deltas > 0 && streamed.trim().length > 0;
    results.push({
      chain: "B 流式", ok, ms,
      usage: res.usage ? `${res.usage.promptTokens}+${res.usage.completionTokens}=${res.usage.totalTokens} tok` : "n/a",
      preview: `deltas=${deltas} ${streamed.slice(0, 80)}`,
    });
    if (!ok) failed = true;
  } catch (err) {
    results.push({ chain: "B 流式", ok: false, ms: Math.round(performance.now() - t0), usage: `deltas=${deltas}`, preview: String(err?.message ?? err).slice(0, 200) });
    failed = true;
  }
}

// --- 报告 ---
console.log(`[provider-smoke] service=${service} model=${model} api=${apiFormat} family=${providerFamily}${baseUrlArg ? ` baseUrl=${baseUrlArg}` : ""}`);
for (const r of results) {
  const mark = r.ok ? "✓" : "✗";
  console.log(`  ${mark} ${r.chain}  ${r.ms}ms  usage=${r.usage}`);
  console.log(`    → ${r.preview.replace(/\n/g, "\\n")}`);
}
if (failed) {
  console.error("[provider-smoke] ✗ 冒烟失败（退出码 1）——传输层/适配器行为与预期不符，禁止据此环境执行 pi 1.0 迁移。");
  process.exit(1);
}
console.log("[provider-smoke] ✓ 冒烟通过——本端点的请求构造/SSE 解析/usage 计量在当前 pi 版本下真实可用。");
