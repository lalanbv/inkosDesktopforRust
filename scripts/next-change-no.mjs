#!/usr/bin/env node
/**
 * 专项编号三源核查（544 号施工图 R40「编号工具化」；544 号实施面）。
 *
 * 背景：396/399、533/534、535/536、539–543、540 双撞、545 四撞——并行会话
 * 同日竞态连续撞号；「开编号+收口双查」是人工纪律，本脚本把它变成一条
 * 命令（可在开工前与收口提交前各跑一次，最后一刻取号）。
 *
 * 三源（544 施工图）：
 *  ① 当日变更记录目录：变更记录文档/<YYYYMMDD>/ 下档名 `NNN_*.md` 的号
 *  ② git log：最近 300 条 commit subject 中的 `（NNN号）`
 *  ③ R 段占用表：最近 commit subject 中 `R\d+` 段号与专项号的配对
 *     （比对施工图/规划文档时人工裁决 R 段归属）
 *
 * 输出：全量号集统计 + 下一可用号 + 当日目录占用 + R 段占用表。
 * 退出码恒 0（核查工具，不是门禁）；撞号靠跑的时机，不靠 exit code。
 */

import { existsSync, readdirSync } from "node:fs";
import { execSync } from "node:child_process";
import { join } from "node:path";

const root = process.cwd();
const docRoot = join(root, "变更记录文档");
const today = new Date().toISOString().slice(0, 10).replaceAll("-", "");

/** ① 变更记录目录全量档名号 → { 号: [档名...] } */
function scanDocNumbers() {
  const numbers = new Map();
  if (!existsSync(docRoot)) return numbers;
  for (const day of readdirSync(docRoot)) {
    const dayDir = join(docRoot, day);
    let entries;
    try {
      entries = readdirSync(dayDir);
    } catch {
      continue;
    }
    for (const name of entries) {
      const match = /^(\d+)_/.exec(name);
      if (match) {
        const n = Number(match[1]);
        (numbers.get(n) ?? numbers.set(n, []).get(n)).push(`${day}/${name}`);
      }
    }
  }
  return numbers;
}

/** ②③ git log subject 中的（NNN号）与 R 段号配对。 */
function scanGitNumbers() {
  let subjects = "";
  try {
    subjects = execSync("git log --pretty=%s -300", { cwd: root, encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
  } catch {
    return { committed: [], rSegments: [] };
  }
  const committed = [];
  const rSegments = [];
  for (const line of subjects.split("\n")) {
    const numberMatch = /（(\d+)号）/.exec(line);
    if (!numberMatch) continue;
    const n = Number(numberMatch[1]);
    committed.push(n);
    for (const rMatch of line.matchAll(/R(\d+[a-z]?)/gi)) {
      rSegments.push([`R${rMatch[1]}`, n]);
    }
  }
  return { committed, rSegments };
}

const docNumbers = scanDocNumbers();
const { committed, rSegments } = scanGitNumbers();

const all = new Set([...docNumbers.keys(), ...committed]);
const next = all.size > 0 ? Math.max(...all) + 1 : 1;

console.log("== 专项编号三源核查（next-change-no.mjs，544 号 R40）==");
console.log(`变更记录目录：${docNumbers.size} 个号；git log：${committed.length} 个（NNN号）`);
console.log(`下一可用专项号：${next}`);

const todayEntries = existsSync(join(docRoot, today)) ? readdirSync(join(docRoot, today)) : [];
console.log(`当日目录 变更记录文档/${today}/：${todayEntries.length === 0 ? "空（无并行会话落档）" : todayEntries.join(", ")}`);

// 双源不一致 = 目录/git 有一边漏记（历史曾发生：档先写、提交并行占号）。
const docOnly = [...docNumbers.keys()].filter((n) => !committed.includes(n)).sort((a, b) => b - a);
if (docOnly.length > 0) {
  console.log(`⚠ 仅在目录、未在 git log：${docOnly.slice(0, 10).join(", ")}${docOnly.length > 10 ? " …" : ""}（可能是纯文档提交或未提交档）`);
}

// R 段占用表（最近优先，去重保首个）。
const seenR = new Set();
const rRows = [];
for (const [r, n] of rSegments) {
  const key = r.toLowerCase();
  if (seenR.has(key)) continue;
  seenR.add(key);
  rRows.push(`${r}=${n}`);
}
if (rRows.length > 0) {
  console.log(`R 段占用（近 300 条提交，人工比对施工图归属）：${rRows.slice(0, 20).join(", ")}`);
}

console.log("提醒：取号后写档前与收口提交前各再跑一次（并行会话竞态窗口只能缩不能闭）。");
