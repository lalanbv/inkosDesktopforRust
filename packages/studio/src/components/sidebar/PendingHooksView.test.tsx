import { afterEach, beforeEach, describe, expect, it } from "vitest";
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

// 996 号：筛选态 localStorage 持久化——ContextDock 开合（visible 条件渲染）/
// ArtifactView 重拉会卸载重建本组件，内部 useState 重置导致「点筛选后列表还原
// 全量」（987 走查备案）。持久化后 remount 即恢复筛选。
// 注：本文件为 node 环境 SSR 测试（renderToString），无 jsdom——装内存
// localStorage stub 走「localStorage 存在」的真实浏览器路径；实现侧的
// typeof 卫兵（node 下回落 "all"）由「defaults to all」例隐式覆盖。
describe("PendingHooksView kind filter persistence (996)", () => {
  const STORAGE_KEY = "inkos.hooksKindFilter";
  const store = new Map<string, string>();

  beforeEach(() => {
    store.clear();
    (globalThis as Record<string, unknown>).localStorage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    };
  });

  afterEach(() => {
    delete (globalThis as Record<string, unknown>).localStorage;
    setAppLanguage("zh");
  });

  it("restores the persisted filter after a remount (987 dock-toggle scenario)", () => {
    // 模拟上一次挂载时用户点过「悬念」筛选
    localStorage.setItem(STORAGE_KEY, "suspense");
    const html = renderToString(<PendingHooksView content={LEDGER} />);
    // 只剩 H01（悬念）核心文本；H02（情感线）核心被筛掉
    expect(html).toContain("镜中世界的来历真相");
    expect(html).not.toContain("苏檀与镜灵的婚约誓言");
  });

  it("falls back to all when the stored value is stale (kind no longer exists)", () => {
    localStorage.setItem(STORAGE_KEY, "bogus-kind");
    const html = renderToString(<PendingHooksView content={LEDGER} />);
    // 回落全量：H01 与 H02 核心都可见
    expect(html).toContain("镜中世界的来历真相");
    expect(html).toContain("苏檀与镜灵的婚约誓言");
  });

  it("defaults to all when nothing is stored", () => {
    const html = renderToString(<PendingHooksView content={LEDGER} />);
    expect(html).toContain("镜中世界的来历真相");
    expect(html).toContain("苏檀与镜灵的婚约誓言");
  });
});
