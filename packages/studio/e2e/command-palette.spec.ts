import { test, expect } from "@playwright/test";

// P1-5/6/7 验收：命令面板（⌘K 打开 / 过滤导航 / 执行跳转 / 最近访问记录）
// 与顶栏伪搜索入口。全部用 page.route 桩掉接口，不依赖真实数据。

const PROJECT_READY = { language: "zh", languageExplicit: true };

async function stubApis(page: import("@playwright/test").Page) {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", (route) => route.fulfill({ json: { books: [] } }));
  await page.route("**/api/v1/interactive-films", (route) => route.fulfill({ json: { films: [] } }));
  await page.route("**/api/v1/daemon", (route) => route.fulfill({ json: { running: false } }));
}

const paletteHotkey = process.platform === "darwin" ? "Meta+k" : "Control+k";

test("command palette navigates, closes on run, and tracks the visit as recent", async ({ page }) => {
  await stubApis(page);
  await page.goto("/#/");

  // ⌘K / Ctrl+K 打开面板；空查询展示推荐组
  await page.keyboard.press(paletteHotkey);
  const dialog = page.locator('[data-slot="dialog-content"]');
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("推荐")).toBeVisible();
  await expect(dialog.getByText("新建长篇小说")).toBeVisible();

  // 输入过滤导航层，回车执行第一条
  await page.keyboard.type("日志");
  await expect(dialog.getByText("前往")).toBeVisible();
  await page.keyboard.press("Enter");

  // 执行后面板关闭，顶栏面包屑切到日志页
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('[data-testid="breadcrumb"]')).toContainText("日志");

  // 再次打开：最近访问组记录了刚才的跳转；Esc 关闭
  await page.keyboard.press(paletteHotkey);
  await expect(dialog.getByText("最近访问")).toBeVisible();
  await expect(dialog.getByText("日志").first()).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("header pseudo-search button opens the palette", async ({ page }) => {
  await stubApis(page);
  await page.goto("/#/");

  await page.locator('[data-testid="command-palette-trigger"]').click();
  await expect(page.locator('[data-slot="dialog-content"]')).toBeVisible();

  // 空查询不出现死胡同文案；推荐组直达「项目设置」
  await expect(page.locator('[data-slot="palette-empty"]')).toHaveCount(0);
  await expect(page.locator('[data-slot="dialog-content"]').getByText("项目设置")).toBeVisible();
});
