import { describe, expect, it } from "vitest";
import { escapeTableCell, restoreTableCell, splitTableRowEscaped } from "../utils/markdown-table.js";

// 822/816 号：markdown 表格转义/还原/拆分单一真源直测——此前仅通过消费者间接测试
//（hook-lifecycle-superseded.test / rollback-legacy-chapter-files.test），
// 本文件补直接单元测试确保共享模块自身行为契约。
describe("markdown-table shared module", () => {
  it("escapeTableCell escapes pipes and converts newlines to <br>", () => {
    expect(escapeTableCell("a|b")).toBe("a\\|b");
    expect(escapeTableCell("a\r\nb")).toBe("a<br>b");
    expect(escapeTableCell("a\nb")).toBe("a<br>b");
    expect(escapeTableCell("  trim  ")).toBe("trim");
    expect(escapeTableCell("plain")).toBe("plain");
  });

  it("restoreTableCell restores <br> variants and escaped pipes", () => {
    expect(restoreTableCell("a<br>b")).toBe("a\nb");
    expect(restoreTableCell("a<BR/>b")).toBe("a\nb");
    expect(restoreTableCell("a<br />b")).toBe("a\nb");
    expect(restoreTableCell("a\\|b")).toBe("a|b");
    expect(restoreTableCell("plain")).toBe("plain");
  });

  it("splitTableRowEscaped skips escaped pipes", () => {
    expect(splitTableRowEscaped("| a\\|b | c |")).toEqual(["", " a\\|b ", " c ", ""]);
    expect(splitTableRowEscaped("| a | b |")).toEqual(["", " a ", " b ", ""]);
    expect(splitTableRowEscaped("| lone")).toEqual(["", " lone"]);
  });

  it("round-trips escape → restore losslessly", () => {
    const original = "A|B\nC";
    expect(restoreTableCell(escapeTableCell(original))).toBe(original);
  });
});
