import { test, expect, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { PAGE_SPEC } from "../src/hooks/use-hash-route";
import type { HashRoute } from "../src/hooks/use-hash-route";
import { A11Y_BOOK, A11Y_FILM, A11Y_PROFILE, A11Y_WORK, seedA11y } from "./fixtures/seed-a11y";

// 877 号：全路由真浏览器 axe 审计（872/873/875 号手工 IAB 审计的自动化固化）。
// - 页面集 = PAGE_SPEC 全部页面（路由单一事实表），hash 由 toHash 生成——873 号
//   曾手写错 4 个 hash 回落首页而审计照样「通过」，此处根除；新增页面未入
//   AUDIT_ROUTES 即覆盖性断言红。
// - 双壳：V1 整栏 Sidebar（新用户默认）+ V2 活动栏（localStorage 显式开）。
// - 断言：除允许清单外零违规（含 moderate）；color-contrast 真浏览器计算。
// - 足时纪律机制化：DOM 静默 600ms 才审（872 号批量中间态假信号教训）。
// - 必红自检：注入已知违规必须被检出（847/848 号纪律：防检测器静默恒绿）。

const require = createRequire(import.meta.url);
const AXE_SOURCE = readFileSync(require.resolve("axe-core/axe.min.js"), "utf-8");

const AUDIT_ROUTES: { readonly [K in HashRoute["page"]]: Extract<HashRoute, { page: K }> } = {
  dashboard: { page: "dashboard" },
  chat: { page: "chat" },
  book: { page: "book", bookId: A11Y_BOOK },
  "book-settings": { page: "book-settings", bookId: A11Y_BOOK },
  "book-timeline": { page: "book-timeline", bookId: A11Y_BOOK },
  analytics: { page: "analytics", bookId: A11Y_BOOK },
  "book-create": { page: "book-create" },
  chapter: { page: "chapter", bookId: A11Y_BOOK, chapterNumber: 1 },
  services: { page: "services" },
  onboarding: { page: "onboarding" },
  "project-settings": { page: "project-settings" },
  translation: { page: "translation" },
  import: { page: "import" },
  "service-detail": { page: "service-detail", serviceId: "custom" },
  work: { page: "work", workId: A11Y_WORK },
  "work-chat": { page: "work-chat", workId: A11Y_WORK, profileId: A11Y_PROFILE },
  play: { page: "play", projectId: A11Y_FILM },
  film: { page: "film", projectId: A11Y_FILM },
  flow: { page: "flow", projectId: A11Y_FILM },
  "film-author": { page: "film-author", projectId: A11Y_FILM },
  "film-studio": { page: "film-studio", projectId: A11Y_FILM },
  radar: { page: "radar" },
  doctor: { page: "doctor" },
  genres: { page: "genres" },
  style: { page: "style" },
  truth: { page: "truth", bookId: A11Y_BOOK },
  daemon: { page: "daemon" },
  logs: { page: "logs" },
};

interface AxeViolation {
  readonly id: string;
  readonly impact: string | null;
  readonly help: string;
  readonly nodes: ReadonlyArray<{ readonly target: ReadonlyArray<string>; readonly summary: string }>;
}

/** 允许清单：空。872 号曾把 region×2（dialog-header 飘在地标外）备案为「Base UI
 *  框架内部结构」——877 号查实是 CommandDialog 把 sr-only 头放在 Dialog Root
 *  直接子节点（关闭态常驻 DOM）的本仓用法问题，已修。变异证明：回退修复后这条
 *  豁免恰好掩护回归（审计照绿），故删除——豁免清单只会过期成掩护面（850 号教训）。
 *  新增豁免必须附定性证据与「回退即红」的反向证明。 */
function isAllowlisted(_violation: AxeViolation): boolean {
  return false;
}

async function waitForQuietDom(page: Page, quietMs = 600, timeoutMs = 10_000): Promise<void> {
  await page.evaluate(([quiet, limit]) => new Promise<void>((resolve) => {
    const finish = () => {
      observer.disconnect();
      clearTimeout(hard);
      resolve();
    };
    let timer = setTimeout(finish, quiet);
    const observer = new MutationObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(finish, quiet);
    });
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    const hard = setTimeout(finish, limit);
  }), [quietMs, timeoutMs] as const);
}

async function auditViolations(page: Page): Promise<AxeViolation[]> {
  await page.addScriptTag({ content: AXE_SOURCE });
  return page.evaluate(async () => {
    type RawNode = { target: unknown[]; failureSummary?: string };
    type RawViolation = { id: string; impact: string | null; help: string; nodes: RawNode[] };
    const axe = (window as unknown as { axe: { run: (ctx: Document, opts: object) => Promise<{ violations: RawViolation[] }> } }).axe;
    const result = await axe.run(document, { resultTypes: ["violations"] });
    return result.violations.map((v) => ({
      id: v.id,
      impact: v.impact,
      help: v.help,
      nodes: v.nodes.map((n) => ({
        target: n.target.map(String),
        summary: (n.failureSummary ?? "").split("\n")[1]?.trim() ?? "",
      })),
    }));
  });
}

function describeViolations(violations: ReadonlyArray<AxeViolation>): string {
  return violations
    .map((v) => `${v.id}(${v.impact}) ×${v.nodes.length}: ${v.help}\n${v.nodes.map((n) => `    ${n.target.join(" ")} | ${n.summary}`).join("\n")}`)
    .join("\n");
}

test.beforeAll(async () => {
  await seedA11y();
});

test("审计清单覆盖 PAGE_SPEC 全部页面（新增页面漏审即红）", () => {
  expect(Object.keys(AUDIT_ROUTES).sort()).toEqual(Object.keys(PAGE_SPEC).sort());
});

test("必红自检：注入的已知违规必须被检出（含真浏览器对比度计算）", async ({ page }) => {
  await page.goto("/#/");
  await expect(page.getByTestId("status-bar")).toBeVisible({ timeout: 20_000 });
  await waitForQuietDom(page);
  await page.evaluate(() => {
    const probe = document.createElement("div");
    probe.setAttribute("data-a11y-probe", "");
    probe.style.cssText = "position:fixed;top:0;left:0;z-index:9999;background:#ffffff;padding:4px";
    probe.innerHTML = '<button type="button"></button><p style="color:#c8c8c8;margin:0">低对比探针文本</p>';
    document.body.appendChild(probe);
  });
  const ids = (await auditViolations(page)).map((v) => v.id);
  expect(ids).toEqual(expect.arrayContaining(["button-name", "color-contrast"]));
});

for (const shell of ["v1", "v2"] as const) {
  test.describe(`a11y 全路由审计（${shell === "v1" ? "V1 默认壳" : "V2 活动栏壳"}）`, () => {
    for (const [pageKey, route] of Object.entries(AUDIT_ROUTES) as Array<[HashRoute["page"], HashRoute]>) {
      test(pageKey, async ({ page }) => {
        await page.addInitScript((target) => {
          // 878 号：默认主题按时段（6-18 点浅、其余深）——夜间跑套件会整站切
          // dark，色卡基线漂移。审计固定浅色主题（877 基线语义）；dark 模式
          // 色卡另行专项审计，不混入本 spec。
          localStorage.setItem("inkos:studio:theme", "light");
          if (target === "v2") localStorage.setItem("inkos:studio:nav-layout-v2", "true");
        }, shell);

        const spec = PAGE_SPEC[pageKey] as { toHash: (route: HashRoute) => string } | null;
        expect(spec, `${pageKey} 缺 PAGE_SPEC 条目`).not.toBeNull();
        await page.goto(`/${spec!.toHash(route)}`);
        await expect(page.getByTestId("status-bar")).toBeVisible({ timeout: 20_000 });
        await waitForQuietDom(page);
        // 覆盖性卫兵（877 首跑教训）：书未注册为 work 时书页渲染「Book … not
        // found」错误卡，axe 只报一个 h1 缺失、其余照常——审计看似通过、实际只
        // 审了错误卡。夹具实体缺失/路由回落首页/页面崩溃都必须红，不得冒充绿。
        const mainText = (await page.locator("main").innerText()).trim();
        expect(mainText, `${pageKey} 主区渲染了错误态而非真实页面`).not.toMatch(/not found|界面遇到了问题|Something went wrong/i);
        if (pageKey !== "dashboard") {
          await expect(page.getByTestId("breadcrumb")).not.toHaveText(/^首页$/);
        }
        const violations = (await auditViolations(page)).filter((v) => !isAllowlisted(v));
        expect(violations, describeViolations(violations)).toEqual([]);
      });
    }
  });
}

// 879 号：dark 主题抽查（chapter/service-detail/doctor——878 号夜间全量暴露
// 的三处 destructive/amber 深底色卡所在页）。主题实现 = dark 类挂 html +
// localStorage 三态 "dark"，不依赖 prefers-color-scheme，headless 下可确定复现。
for (const [pageKey, route] of [
  ["chapter", { page: "chapter", bookId: A11Y_BOOK, chapterNumber: 1 }],
  ["service-detail", { page: "service-detail", serviceId: "custom" }],
  ["doctor", { page: "doctor" }],
] as Array<[HashRoute["page"], HashRoute]>) {
  test(`dark 主题抽查：${pageKey}`, async ({ page }) => {
    await page.addInitScript((target) => {
      localStorage.setItem("inkos:studio:theme", "dark");
      localStorage.setItem("inkos:studio:nav-layout-v2", "true");
      void target;
    }, pageKey);
    const spec = PAGE_SPEC[pageKey] as { toHash: (route: HashRoute) => string } | null;
    await page.goto(`/${spec!.toHash(route)}`);
    await expect(page.getByTestId("status-bar")).toBeVisible({ timeout: 20_000 });
    await waitForQuietDom(page);
    await page.addScriptTag({ content: AXE_SOURCE });
    await page.evaluate(() => {
      // 必红探针注入 main 内（region 由地标覆盖）；违规样本自带白底浅灰字
      //（contrast 违规）+ 无名按钮（button-name）——dark 页面上白底容器在
      // 根节点会自带 region 违规污染断言（879 首跑教训）。
      const probe = document.createElement("div");
      probe.setAttribute("data-a11y-probe", "");
      // fixed 定位回视口内：axe color-contrast 跳过视口外元素（main 尾部 append
      // 在页面底外会被跳过，只剩 button-name——879 首跑教训）。白底子元素供
      // contrast 违规样本（深色页面上白底浅灰字 1.67）。
      probe.style.cssText = "position:fixed;top:0;left:0;z-index:9999";
      probe.innerHTML = '<button type="button" style="background:#ffffff;padding:4px"></button>'
        + '<p style="background:#ffffff;color:#c8c8c8;margin:0;padding:4px">低对比探针文本</p>';
      document.querySelector("main")?.appendChild(probe);
    });
    // 探针验证与干净审计分离：先断言检出（dark 检测器活性），移除探针后再
    // 全量断言零违规——探针违规样本混进零断言会让 dark 抽查永远红（879 首跑）。
    const probeIds = await page.evaluate(async () => {
      const axe = (window as unknown as { axe: { run: (ctx: Document, opts: object) => Promise<{ violations: Array<{ id: string }> }> } }).axe;
      return (await axe.run(document, { resultTypes: ["violations"] })).violations.map((v) => v.id);
    });
    expect(probeIds).toEqual(expect.arrayContaining(["button-name", "color-contrast"]));
    await page.evaluate(() => document.querySelector('[data-a11y-probe]')?.remove());
    await waitForQuietDom(page);
    const violations = (await auditViolations(page)).filter((v) => !isAllowlisted(v));
    expect(violations, describeViolations(violations)).toEqual([]);
  });
}
