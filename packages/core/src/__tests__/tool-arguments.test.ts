import { describe, expect, it } from "vitest";
import { Type } from "@sinclair/typebox";
import { toolArgumentIssues } from "../agent/tool-arguments.js";

// 1021 号：toolArgumentIssues 适配层直接契约单测（1019 重写为 typebox 1.x
// 2020-12 错误形态双版本归一适配——本测试锁输出契约与 valueAtPath 边界，
// 升级态回归面：真实 Value.Errors 产物走全链）。
describe("toolArgumentIssues（1019 适配层契约）", () => {
  it("type 错误：path/expectedType/actualType/instruction 全面重建", () => {
    const schema = Type.Object({
      chapters: Type.Array(Type.String()),
    });
    const issues = toolArgumentIssues(schema, { chapters: "not-an-array" });
    const chapter = issues.find(issue => issue.path === "/chapters");
    expect(chapter).toMatchObject({
      path: "/chapters",
      expectedType: "array",
      actualType: "string",
    });
    // 字符串提交原生 JSON 字段的引导语（LLM 自纠提示契约）
    expect(chapter?.instruction).toContain("native JSON array");
  });

  it("Union-of-Literal：allowedValues 按路径聚合（1.x 多条 const 重建 0.x enum 语义）", () => {
    const schema = Type.Object({
      state: Type.Union([Type.Literal("draft"), Type.Literal("ready")]),
    });
    const issues = toolArgumentIssues(schema, { state: "bogus" });
    const state = issues.find(issue => issue.path === "/state");
    expect(state?.actualType).toBe("string");
    expect(state?.allowedValues).toEqual(["draft", "ready"]);
  });

  it("嵌套路径与数组下标：valueAtPath 按 RFC 6901 分段定位 received", () => {
    const schema = Type.Object({
      chapters: Type.Array(
        Type.Object({ title: Type.Integer({ minimum: 1 }) }),
      ),
    });
    const issues = toolArgumentIssues(schema, {
      chapters: [{ title: "should-be-int" }, { title: 3 }],
    });
    // 仅第 1 元素的 title 越界——instancePath /chapters/0/title 精确定位
    const bad = issues.find(issue => issue.path === "/chapters/0/title");
    expect(bad?.actualType).toBe("string");
    const good = issues.find(issue => issue.path === "/chapters/1/title");
    expect(good).toBeUndefined();
  });

  it("顶层错误：空 instancePath 收到完整根值", () => {
    const schema = Type.String();
    const issues = toolArgumentIssues(schema, 42);
    expect(issues.length).toBeGreaterThan(0);
    expect(issues[0].path).toBe("");
    expect(issues[0].actualType).toBe("number");
  });

  it("合法输入零错误", () => {
    const schema = Type.Object({
      chapters: Type.Array(Type.String()),
      state: Type.Union([Type.Literal("draft"), Type.Literal("ready")]),
    });
    expect(toolArgumentIssues(schema, { chapters: ["a"], state: "draft" })).toEqual([]);
  });
});
