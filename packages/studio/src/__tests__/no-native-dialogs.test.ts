//! 1655 号：原生对话框 API 清零守卫（1650-1655 批三收官机制化）——wry 壳
//! 内 prompt→null/confirm→false/alert→静默（WKUIDelegate 无三面板实现），
//! 全部交互必须走 React 化组件/页内通知条。扫描 studio src 全量源码
//! （排除本测试与 PromptDialog/ConfirmDialog 组件自身注释/字符串面），
//! 任何新的 window.alert/prompt/confirm 或裸 alert( 调用立即红。
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const studioSrc = resolve(here, "..");

function listSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    if (name.startsWith(".") || name === "__tests__" || name.includes(".test.")) continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...listSourceFiles(path));
    else if (/\.(ts|tsx)$/u.test(name)) out.push(path);
  }
  return out;
}

/** 全局调用形态（window.alert / 裸 alert(）——注释里的提及剥除后检测。 */
function stripCommentsAndStrings(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//gu, "")
    .replace(/\/\/[^\n]*/gu, "")
    .replace(/`(?:\\.|[^`\\])*`/gsu, "")
    .replace(/"(?:\\.|[^"\\])*"/gsu, "")
    .replace(/'(?:\\.|[^'\\])*'/gsu, "");
}

describe("原生对话框清零守卫（1650-1655 号）", () => {
  it("studio src 无 window.alert/prompt/confirm 与裸 alert( 调用", () => {
    const offenders: string[] = [];
    for (const file of listSourceFiles(studioSrc)) {
      const rel = relative(studioSrc, file);
      const code = stripCommentsAndStrings(readFileSync(file, "utf-8"));
      if (/\bwindow\s*\.\s*(alert|prompt|confirm)\s*\(/u.test(code)) {
        offenders.push(`${rel}: window.* 原生对话框`);
      }
      // 裸 alert( / confirm( / prompt(：排除 PromptDialog/ConfirmDialog 组件名。
      if (/(?<![\w.])(alert|prompt|confirm)\s*\(/u.test(code)) {
        offenders.push(`${rel}: 裸 ${/prompt|confirm|alert/.exec(code)?.[0]} 调用`);
      }
    }
    expect(offenders, `原生对话框回归（wry 壳内全哑）:\n${offenders.join("\n")}`).toEqual([]);
  });
});
