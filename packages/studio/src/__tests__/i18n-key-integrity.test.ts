import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * 727 号：i18n 键完整性静态扫描回归锁——组件消费的 t("…")/tr("…") 字面
 * 键必须都在词典（strings）内，防止未来新增调用时裸键名暴露给用户。
 *
 * 扫描口径：
 * - 消费面 = src 下 .ts/.tsx 的字面量 `t("…")` / `tr("…")`（动态键模板串
 *   不在本口径——那些键经 map/常量间接消费，属词典维护面而非完整性面）；
 * - 词典面 = use-i18n.ts 的 `"key": {` 条目形态（zh/en 多行对象；strings
 *   常量非导出，词典键从源码文本提取——提取器对准定义形态，单行 grep
 *   会假阳性，720 号信号扫描法同类教训）。
 */
const SRC_DIR = join(import.meta.dirname ?? ".", "..");
const DICT_FILE = join(SRC_DIR, "hooks", "use-i18n.ts");

function collectSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      if (name === "node_modules" || name === "__tests__") continue;
      out.push(...collectSourceFiles(full));
    } else if ((name.endsWith(".ts") || name.endsWith(".tsx")) && full !== DICT_FILE) {
      out.push(full);
    }
  }
  return out;
}

const USED_KEYS = new Set<string>();
for (const file of collectSourceFiles(SRC_DIR)) {
  const text = readFileSync(file, "utf-8");
  for (const match of text.matchAll(/\b(?:t|tr)\("([a-z]+\.[a-zA-Z0-9_.]+)"/g)) {
    USED_KEYS.add(match[1]!);
  }
}

const dictText = readFileSync(DICT_FILE, "utf-8");
const DICT_KEYS = new Set(
  [...dictText.matchAll(/^\s*"([a-z]+\.[a-zA-Z0-9_.]+)":\s*\{/gm)].map((m) => m[1]!),
);

describe("i18n key integrity (727)", () => {
  it("消费的字面键全部在词典内（防裸键名暴露）", () => {
    const missing = [...USED_KEYS].filter((key) => !DICT_KEYS.has(key));
    expect(missing, `词典缺失 ${missing.length} 键：${missing.join(", ")}`).toEqual([]);
  });
});

