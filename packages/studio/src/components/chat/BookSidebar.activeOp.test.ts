import { describe, expect, it } from "vitest";
import type { SSEMessage } from "../../hooks/use-sse";
import { deriveActiveOp } from "./BookSidebar";

// 1003 号：写作活动指示全量重放——消除「只看最后一条」增量形（999 同族补位）。
// PanelView 挂在移动端抽屉 BookSidebarToggle 的 `{open && ...}` 条件挂载下：
// 抽屉开合卸载期间 store 累积的中间事件被旧增量形跳过——write:start 后落到
// log 等未映射类型时重挂载指示丢失。全量重放幂等且卸载期事件不丢。
// 分形语义：依赖累积历史的主场景在 NEUTER（只看最后一条）下必红；单事件/
// 终态收敛例恒绿。
function ev(event: string, data?: unknown): SSEMessage {
  return { event, data } as SSEMessage;
}
function toolStart(tool: string): SSEMessage {
  return ev("tool:start", { tool });
}

describe("deriveActiveOp 全量重放（1003）", () => {
  it("卸载期累积流重放：write:start 落进 log 等未映射事件后指示不丢（主场景）", () => {
    // 旧增量形只看最后一条 log → null（指示丢失）；全量重放保持 write。
    const messages = [
      ev("write:start"),
      toolStart("mcp__inkos__write_chapters"),
      ev("log", { message: "准备章节输入" }),
      ev("log", { message: "撰写章节草稿" }),
    ];
    expect(deriveActiveOp(messages)).toBe("write");
  });

  it("audit 累积流：review_chapter 后接 log 重放保持 audit", () => {
    const messages = [
      ev("write:start"),
      ev("write:complete"),
      toolStart("pipeline__review_chapter"),
      ev("log", { message: "审校进行中" }),
    ];
    expect(deriveActiveOp(messages)).toBe("audit");
  });

  it("revise：revise_chapter 单事件解析命名空间后缀", () => {
    expect(deriveActiveOp([toolStart("ns__revise_chapter")])).toBe("revise");
  });

  it("write:complete 收敛清空（终态不复活）", () => {
    const messages = [ev("write:start"), ev("log", { message: "x" }), ev("write:complete")];
    expect(deriveActiveOp(messages)).toBeNull();
  });

  it("tool:end 清空活动操作", () => {
    const messages = [ev("write:start"), toolStart("x__write_chapters"), ev("tool:end")];
    expect(deriveActiveOp(messages)).toBeNull();
  });

  it("idle：空流与纯 log 流均为 null", () => {
    expect(deriveActiveOp([])).toBeNull();
    expect(deriveActiveOp([ev("log", { message: "x" })])).toBeNull();
  });

  it("幂等：同一流重放两次结果一致", () => {
    const messages = [ev("write:start"), ev("log", { message: "x" })];
    expect(deriveActiveOp(messages)).toBe(deriveActiveOp(messages));
    expect(deriveActiveOp(messages)).toBe("write");
  });
});
