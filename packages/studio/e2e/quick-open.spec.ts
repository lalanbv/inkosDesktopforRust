import { test, expect } from "@playwright/test";

// P2-4/5/6 验收：Cmd+P 快速打开（书名/章节号直达）、⌘/ 速查弹层与注册表一致性。
// 接口全部走 page.route 桩，不依赖真实数据。

const PROJECT_READY = { language: "zh", languageExplicit: true };
const quickOpenHotkey = process.platform === "darwin" ? "Meta+p" : "Control+p";
const cheatSheetHotkey = process.platform === "darwin" ? "Meta+/" : "Control+/";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", (route) =>
    route.fulfill({ json: { books: [{ id: "qo-book", title: "山河志" }] } }),
  );
  await page.route("**/api/v1/books/qo-book", (route) =>
    route.fulfill({
      json: {
        book: { id: "qo-book", title: "山河志" },
        chapters: [
          { number: 1, title: "启程" },
          { number: 12, title: "夜袭" },
        ],
        nextChapter: 2,
      },
    }),
  );
  await page.route("**/api/v1/interactive-films", (route) => route.fulfill({ json: { films: [] } }));
});

test("quick open jumps to a book by title and a chapter by number", async ({ page }) => {
  await page.goto("/#/");

  // ⌘P 打开快速打开（与 ⌘K 命令面板分离的实例）
  await page.keyboard.press(quickOpenHotkey);
  const dialog = page.locator('[data-slot="dialog-content"]');
  await expect(dialog).toBeVisible({ timeout: 10_000 });

  // 书名直达（章节条目的 detail 也会显示书名，取首项即书籍条目）
  await page.keyboard.type("山河");
  await expect(dialog.getByText("山河志").first()).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("breadcrumb")).toContainText("山河志", { timeout: 10_000 });

  // 章节号直达：输入 12 命中「夜袭」，回车后面包屑出现 第12章
  await page.keyboard.press(quickOpenHotkey);
  await page.keyboard.type("12");
  await expect(dialog.getByText("夜袭")).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("breadcrumb")).toContainText("第12章", { timeout: 10_000 });
});

test("cheat sheet opens with mod+/ and lists registered combos, closes on Escape", async ({ page }) => {
  await page.goto("/#/");

  await page.keyboard.press(cheatSheetHotkey);
  const sheet = page.locator('[data-slot="hotkey-cheatsheet"]');
  await expect(sheet).toBeVisible({ timeout: 10_000 });

  // 注册表中的三个全局键全部出现（与速查同源）
  await expect(sheet.getByText("⌘K").or(sheet.getByText("Ctrl+K"))).toBeVisible();
  await expect(sheet.getByText("⌘P").or(sheet.getByText("Ctrl+P"))).toBeVisible();
  await expect(sheet.getByText("⌘/").or(sheet.getByText("Ctrl+/"))).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
});
