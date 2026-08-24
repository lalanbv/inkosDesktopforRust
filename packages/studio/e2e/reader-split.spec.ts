import { test, expect } from "@playwright/test";

// 读写对照分屏（166 号）：ChapterReader 右侧只读对照栏——开/关、换章、
// 拖宽记忆。全部用 page.route 桩掉接口，不依赖真实数据。

const PROJECT_READY = { language: "zh", languageExplicit: true };

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", (route) =>
    route.fulfill({ json: { books: [{ id: "b1", title: "山河志" }] } }));
  await page.route("**/api/v1/interactive-films", (route) => route.fulfill({ json: { films: [] } }));
  await page.route("**/api/v1/daemon", (route) => route.fulfill({ json: { running: false } }));
});

test("reader split pane opens with adjacent chapter, switches and closes", async ({ page }) => {
  await page.route("**/api/v1/books/b1", (route) =>
    route.fulfill({ json: { book: { id: "b1" }, chapters: [{ number: 1, title: "启程" }, { number: 2, title: "夜行" }], nextChapter: 3 } }));
  await page.route("**/api/v1/books/b1/chapters/1", (route) =>
    route.fulfill({ json: { chapterNumber: 1, filename: "c1.md", content: "# 第 1 章\n\n第一章正文首段。\n\n第一章正文次段。" } }));
  await page.route("**/api/v1/books/b1/chapters/2", (route) =>
    route.fulfill({ json: { chapterNumber: 2, filename: "c2.md", content: "# 第 2 章\n\n第二章对照正文。" } }));

  await page.goto("/#/book/b1");
  // ⌘P 直达章节 1（与 focus-mode 用例同路径）
  await page.keyboard.press(process.platform === "darwin" ? "Meta+p" : "Control+p");
  const input = page.locator('[data-slot="command-input"]');
  await expect(input).toBeVisible({ timeout: 10_000 });
  await page.keyboard.type("启程");
  await expect(page.getByRole("option", { name: /启程/ })).toBeVisible({ timeout: 10_000 });
  await page.keyboard.press("Enter");
  await expect(page.locator("article p").first()).toBeVisible({ timeout: 15_000 });

  // 初始无对照栏；开启后默认对照上一章（章节 1 无前章 → 空态提示）
  await expect(page.locator('[data-testid="reader-split-pane"]')).toHaveCount(0);
  await page.locator('[data-testid="reader-split-toggle"]').click();
  const pane = page.locator('[data-testid="reader-split-pane"]');
  await expect(pane).toBeVisible();
  await expect(pane).toContainText("输入章号或点 ›");

  // 跳章输入 2 → 对照栏加载第二章正文（只读渲染）
  await page.locator('[data-testid="reader-split-jump"]').fill("2");
  await page.locator('[data-testid="reader-split-jump"]').press("Enter");
  await expect(pane.locator("article p").first()).toContainText("第二章对照正文", { timeout: 10_000 });

  // 下一章按钮推进到 3（未桩 → 错误面可见，不崩）
  await page.getByLabel("下一章").click();
  await expect(pane).toContainText("对照章加载失败", { timeout: 10_000 });

  // 关闭：toggle 再点 → 对照栏消失
  await page.locator('[data-testid="reader-split-toggle"]').click();
  await expect(page.locator('[data-testid="reader-split-pane"]')).toHaveCount(0);
});

test("reader split width drag persists across reopen", async ({ page }) => {
  await page.route("**/api/v1/books/b1", (route) =>
    route.fulfill({ json: { book: { id: "b1" }, chapters: [{ number: 1, title: "启程" }, { number: 2, title: "夜行" }], nextChapter: 3 } }));
  await page.route("**/api/v1/books/b1/chapters/1", (route) =>
    route.fulfill({ json: { chapterNumber: 1, filename: "c1.md", content: "# 第 1 章\n\n正文。" } }));
  await page.route("**/api/v1/books/b1/chapters/2", (route) =>
    route.fulfill({ json: { chapterNumber: 2, filename: "c2.md", content: "# 第 2 章\n\n对照。" } }));

  await page.goto("/#/book/b1");
  await page.keyboard.press(process.platform === "darwin" ? "Meta+p" : "Control+p");
  const input = page.locator('[data-slot="command-input"]');
  await expect(input).toBeVisible({ timeout: 10_000 });
  await page.keyboard.type("夜行");
  await expect(page.getByRole("option", { name: /夜行/ })).toBeVisible({ timeout: 10_000 });
  await page.keyboard.press("Enter");
  await expect(page.locator("article p").first()).toBeVisible({ timeout: 15_000 });

  await page.locator('[data-testid="reader-split-toggle"]').click();
  const pane = page.locator('[data-testid="reader-split-pane"]');
  await expect(pane).toBeVisible();
  // 章节 2 的对照章 = 上一章 1
  await expect(pane.locator("article p").first()).toContainText("正文。", { timeout: 10_000 });

  // 拖拽分隔条收窄对照栏（贴右缘：左移指针 = 增宽）→ 宽度变化。
  // 分隔条在长稿深处：先滚入视口（y 超出 720 视口时鼠标事件不命中）。
  const divider = page.locator('[data-testid="reader-split-divider"]');
  await divider.scrollIntoViewIfNeeded();
  const box = await divider.boundingBox();
  const before = await pane.evaluate((el) => el.getBoundingClientRect().width);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x - 120, box.y + box.height / 2, { steps: 4 });
  await page.mouse.up();
  const after = await pane.evaluate((el) => el.getBoundingClientRect().width);
  expect(after).toBeGreaterThan(before);

  // 关闭再开：宽度经 localStorage 记忆保持
  await page.locator('[data-testid="reader-split-toggle"]').click();
  await expect(page.locator('[data-testid="reader-split-pane"]')).toHaveCount(0);
  await page.locator('[data-testid="reader-split-toggle"]').click();
  await expect(page.locator('[data-testid="reader-split-pane"]')).toBeVisible();
  const reopened = await page.locator('[data-testid="reader-split-pane"]').evaluate((el) => el.getBoundingClientRect().width);
  expect(Math.abs(reopened - after)).toBeLessThan(2);
});
