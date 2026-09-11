import { z } from "zod";

/**
 * 防呆写出方言（G7c/332 号，WNW v7 防呆方言的采纳版）。
 *
 * 真相文件与设定文件是作者可手改的真源——写出格式必须防呆：一格写坏全废
 * 的行内嵌套、盲目插值的引号（值含 `"` 即破）、混入 BOM 的不可见脏字节，
 * 都是历史上真实出现过的故障面。本模块把这些固化为一套机器可校验的方言：
 *
 * 1. **平铺 front matter**：meta 块一律 `---` 分隔 + 顶层 `key: value` 单行标量，
 *    禁嵌套缩进（对齐 328 号不采纳清单"大一统 YAML 反面教材"教训）。
 * 2. **危险值自动加引号**：含引号/反斜杠/竖线、前后空白、`#`/`-`/`|` 等 YAML
 *    指示符开头、含 `: `/行尾 `:`/` #`、空串、布尔/数字形——一律双引号包裹并转义。
 * 3. **块列表**：`- ` 前缀每条一行，逐条过危险值判定。
 * 4. **UTF-8 无 BOM**：写出前剥除源内容携带的前导 BOM（读取侧各 parser 已各自
 *    剥除；本模块管写出侧，LLM 输出/用户素材不再把 BOM 带进真相文件）。
 *
 * 双端契约：`engine-rs/src/utils/truth_dialect.rs` 为 1:1 镜像，共享向量
 * `src/__tests__/golden/truth-dialect-vectors.json`（TS 断言
 * `golden-truth-dialect.test.ts`；Rust 差分 `tests/golden_truth_dialect_diff.rs`）。
 */

export const TRUTH_DIALECT_CONTRACT_SCHEMA = z.object({
  version: z.literal(1),
  dialect: z.literal("inkos-truth-dialect"),
  rules: z.array(z.string()).min(1),
});

export type TruthDialectContract = z.infer<typeof TRUTH_DIALECT_CONTRACT_SCHEMA>;

export const TRUTH_DIALECT_CONTRACT: TruthDialectContract =
  TRUTH_DIALECT_CONTRACT_SCHEMA.parse({
    version: 1,
    dialect: "inkos-truth-dialect",
    rules: [
      "flat-single-line-scalars-only",
      "dangerous-values-double-quoted-and-escaped",
      "block-list-dash-prefix-per-line",
      "strip-leading-utf8-bom-before-write",
    ],
  });

/** 剥除一个前导 BOM（\\uFEFF）；只剥第一个，中间出现的 BOM 是内容不动。 */
export function stripUtf8Bom(content: string): string {
  return content.charCodeAt(0) === 0xfeff ? content.slice(1) : content;
}

/** 平铺为单行标量：换行（CRLF/CR/LF）与制表符折叠为单空格，去首尾空白。 */
export function flattenTruthScalar(value: string): string {
  return value
    .replace(/\r\n?/g, "\n")
    .replace(/\n/g, " ")
    .replace(/\t/g, " ")
    .trim();
}

/** 判定平铺后的标量是否必须加引号（YAML 指示符/歧义形/转义字符等）。 */
export function isDangerousTruthValue(flat: string): boolean {
  if (flat === "") return true;
  if (/^[\-?:!&*[\]{}>|%#`"'@,]/.test(flat)) return true;
  if (flat.includes(": ")) return true;
  if (flat.endsWith(":")) return true;
  if (flat.includes(" #")) return true;
  if (/["\\|]/.test(flat)) return true;
  if (/^(true|false|null|yes|no|on|off|~)$/i.test(flat)) return true;
  if (/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(flat)) return true;
  return false;
}

/** 双引号包裹 + 反斜杠/双引号转义。入参须已平铺。 */
export function quoteTruthValue(flat: string): string {
  return `"${flat.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

/** 方言标量渲染：平铺 → 危险判定 →（按需）转义加引号。 */
export function renderTruthValue(value: string): string {
  const flat = flattenTruthScalar(value);
  return isDangerousTruthValue(flat) ? quoteTruthValue(flat) : flat;
}

/**
 * 平铺 meta 块：`---` 开头、顶层 `key: value` 每条一行。
 * key 是代码控制的标识符（仅平铺不判危）；value 走完整方言渲染。
 * 调用侧自行决定与正文的空行衔接（本函数不带前导换行）。
 */
export function renderFlatMetaBlock(
  entries: ReadonlyArray<readonly [string, string]>,
): string {
  return [
    "---",
    ...entries.map(([key, value]) => `${flattenTruthScalar(key)}: ${renderTruthValue(value)}`),
  ].join("\n");
}

/** 块列表：`- ` 前缀每条一行，逐条过方言标量渲染；空列表产出 `(无)`。 */
export function renderTruthBlockList(items: ReadonlyArray<string>, emptyMarker = "(无)"): string {
  if (items.length === 0) return emptyMarker;
  return items.map((item) => `- ${renderTruthValue(item)}`).join("\n");
}
