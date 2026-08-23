import { test, expect } from "@playwright/test";

// P4-1/P4-2 验收:专注模式(⌘⇧F 进/Esc 出/chrome 隐藏/打字机段聚焦)、
// 密度档(命令面板切换 + 紧凑行距生效)、reduced-motion 模拟下动效瞬时。

const PROJECT_READY = { language: "zh", languageExplicit: true };
const focusHotkey = process.platform === "darwin" ? "Meta+Shift+f" : "Control+Shift+f";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", (route) =>
    route.fulfill({ json: { books: [{ id: "b1", title: "山河志" }] } }));
  await page.route("**/api/v1/interactive-films", (route) => route.fulfill({ json: { films: [] } }));
  await page.route("**/api/v1/daemon", (route) => route.fulfill({ json: { running: false } }));
});

test("focus mode hides nav chrome, keeps status bar, exits on Esc", async ({ page }) => {
  await page.goto("/#/");

  await expect(page.getByTestId("status-bar")).toBeVisible({ timeout: 10_000 });
  const sidebar = page.locator("aside").first();
  await expect(sidebar).toBeVisible();

  await page.keyboard.press(focusHotkey);
  // chrome 隐藏:侧栏/标签条消失;状态栏保留
  await expect(sidebar).toBeHidden();
  await expect(page.getByTestId("tab-strip")).toBeHidden();
  await expect(page.getByTestId("status-bar")).toBeVisible();
  // 根节点带 focus-mode(低对比变量生效标记)
  await expect(page.locator(".focus-mode")).toHaveCount(1);

  // Esc 退出
  await page.keyboard.press("Escape");
  await expect(sidebar).toBeVisible();
  await expect(page.locator(".focus-mode")).toHaveCount(0);
});

test("typewriter mode dims non-active paragraphs on the chapter reader", async ({ page }) => {
  await page.route("**/api/v1/books/b1", (route) =>
    route.fulfill({ json: { book: { id: "b1" }, chapters: [{ number: 1, title: "启程" }], nextChapter: 2 } }));
  await page.route("**/api/v1/books/b1/chapters/1", (route) =>
    route.fulfill({ json: { chapterNumber: 1, filename: "c1.md", content: "# 第 1 章\n\n第一段正文。\n\n第二段正文。\n\n第三段正文。" } }));

  await page.goto("/#/book/b1");
  // ⌘P 直达章节 1
  await page.keyboard.press(process.platform === "darwin" ? "Meta+p" : "Control+p");
  const input = page.locator('[data-slot="command-input"]');
  await expect(input).toBeVisible({ timeout: 10_000 });
  await page.keyboard.type("启程");
  await expect(page.getByRole("option", { name: /启程/ })).toBeVisible({ timeout: 10_000 });
  await page.keyboard.press("Enter");
  await expect(page.locator("article p").first()).toBeVisible({ timeout: 15_000 });

  // 进入专注 → 点击第二段:首段被压暗,第二段全显
  await page.keyboard.press(focusHotkey);
  const paragraphs = page.locator("article p");
  await paragraphs.nth(1).click();
  await expect(paragraphs.nth(0)).toHaveClass(/typewriter-dim/);
  await expect(paragraphs.nth(1)).not.toHaveClass(/typewriter-dim/);
});

test("density compact via command palette shrinks list row height", async ({ page }) => {
  await page.goto("/#/");

  const row = page.locator("aside").getByRole("button", { name: "题材" });
  await expect(row).toBeVisible({ timeout: 10_000 });
  const comfortable = (await row.boundingBox())!.height;

  // 命令面板切紧凑
  await page.keyboard.press(process.platform === "darwin" ? "Meta+k" : "Control+k");
  const input = page.locator('[data-slot="command-input"]');
  await expect(input).toBeVisible();
  await input.fill("紧凑");
  await page.getByRole("option", { name: /紧凑/ }).click();
  await expect(page.locator(".density-compact")).toHaveCount(1);

  // 轮询等待布局收敛(类应用与测量间的渲染竞态用 poll 吸收)
  await expect.poll(async () => (await row.boundingBox())!.height).toBeLessThan(comfortable);
});

test("notification center bell opens a non-blocking panel (P4-3)", async ({ page }) => {
  await page.goto("/#/");

  const bell = page.getByTestId("notification-bell");
  await expect(bell).toBeVisible({ timeout: 10_000 });
  // 无通知:面板显示空态;再次点击关闭
  await bell.click();
  await expect(page.getByTestId("notification-panel")).toBeVisible();
  await expect(page.getByText("暂无通知")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("notification-panel")).toHaveCount(0);
});
