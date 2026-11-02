/**
 * 816 号：markdown 表格单元格的转义/还原单一真源（TS 侧）——与 Rust
 * `engine-rs/src/utils/markdown_table.rs` 逐字同集。
 *
 * 契约：渲染 `|` → `\|`、`\r\n`/`\n` → `<br>`、trim；解析按未转义 `|` 拆分，单元格
 * `<br>`（三形态、忽略大小写）→ 换行、`\|` → `|`。
 *
 * 历史：曾以物理拷贝散布 state-projections / story-markdown 两处（forecast/render 的
 * 「换行→空格」是另一语义，不在此列），810/811/812 各修一处后收敛——新渲染/解析点
 * 必须走本模块。
 */

/** 渲染：单元格转义（竖线 → `\|`、换行 → `<br>`、首尾空白 trim）。 */
export function escapeTableCell(value: string | number): string {
  return String(value).replace(/\|/g, "\\|").replace(/\r?\n/g, "<br>").trim();
}

/** 解析：单元格回读还原（`<br>` 三形态 → 换行、`\|` → `|`）。 */
export function restoreTableCell(cell: string): string {
  return cell.replace(/<br\s*\/?>/gi, "\n").replace(/\\\|/g, "|");
}

/** 按未转义的 `|` 拆分表格行（`\|` 不作分隔，留给 `restoreTableCell` 还原）。 */
export function splitTableRowEscaped(line: string): string[] {
  return line.split(/(?<!\\)\|/);
}
