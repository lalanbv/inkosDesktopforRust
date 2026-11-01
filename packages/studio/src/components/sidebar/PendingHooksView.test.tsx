import { afterEach, describe, expect, it } from "vitest";
import { renderToString } from "react-dom/server";
import { setAppLanguage } from "../../lib/app-language";
import { PendingHooksView } from "./PendingHooksView";

// 814 号：伏笔侧栏终态徽标补全（resolved 已回收 / superseded 已撤回）+ 撤回 notes 多行与
// 含竖线转义在 UI 端到端还原（805 撤回语义 → 810/812 转义 → 811/812 解析还原 → 本视图按行展示）。
const LEDGER = [
  "| hook_id | 起始章节 | 类型 | 状态 | 最近推进 | 预期回收 | 回收节奏 | 上游依赖 | 回收卷 | 核心 | 半衰期 | 升级 | 备注 | 分类 |",
  "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  "| H01 | 1 | 身世 | progressing | 3 | 第10章 | slow-burn | 无 | 第一卷 | 是 | 10 | 是 | 镜中世界的来历真相。 | 悬念 |",
  "| H02 | 2 | 情感线 | resolved | 3 | 第8章 | near-term | 无 | 第一卷 | 否 | 8 | 否 | 苏檀与镜灵的婚约誓言。 | 情感 |",
  "| H03 | 2 | 信物 | superseded | 3 | 第6章 | near-term | H01 | 第一卷 | 否 | 6 | 否 | 母亲留下的碎镜。<br>作者修订撤回：并入 H01\\|H04。 | 物品 |",
].join("\n");

describe("PendingHooksView status badges (814)", () => {
  afterEach(() => setAppLanguage("zh"));

  it("renders terminal-state badges and keeps the withdrawn note visible with its authority line", () => {
    setAppLanguage("zh");
    const html = renderToString(<PendingHooksView content={LEDGER} />);
    expect(html).toContain("活跃");
    expect(html).toContain("已回收");
    expect(html).toContain("已撤回");
    // 撤回依据与竖线均还原后展示（<br> → 换行按 whitespace-pre-line 渲染，\| → |）。
    expect(html).toContain("作者修订撤回：并入 H01|H04。");
    expect(html).not.toContain("&lt;br&gt;");
    expect(html).toContain("whitespace-pre-line");
  });

  it("localises the badges", () => {
    setAppLanguage("en");
    const html = renderToString(<PendingHooksView content={LEDGER} />);
    expect(html).toContain("Resolved");
    expect(html).toContain("Withdrawn");
    expect(html).not.toContain("已撤回");
  });
});
