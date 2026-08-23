import { test, expect } from "@playwright/test";

// P3-4 验收：底部面板（Cmd+J）两 tab 与滚动跟随按钮；右侧 dock（Cmd+Shift+D）
// 在书籍页可用；两者并存时底部面板常规高度 < 40% 视口高（不挤压编辑区）；
// 开合状态按页记忆。

const PROJECT_READY = { language: "zh", languageExplicit: true };
const bottomHotkey = process.platform === "darwin" ? "Meta+j" : "Control+j";
const dockHotkey = process.platform === "darwin" ? "Meta+Shift+d" : "Control+Shift+d";

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", (route) =>
    route.fulfill({ json: { books: [{ id: "b1", title: "山河志" }] } }));
  await page.route("**/api/v1/interactive-films", (route) => route.fulfill({ json: { films: [] } }));
  await page.route("**/api/v1/daemon", (route) => route.fulfill({ json: { running: false } }));
  await page.route("**/api/v1/logs", (route) => route.fulfill({ json: { entries: [
    { level: "info", tag: "test", message: "面板验收日志行", timestamp: "12:00:00" },
  ] } }));
});

test("bottom panel toggles via Cmd+J, switches tabs, and stays under 40% viewport height", async ({ page }) => {
  await page.goto("/#/");

  // 默认收起
  await expect(page.getByTestId("bottom-panel")).toHaveCount(0);

  await page.keyboard.press(bottomHotkey);
  const panel = page.getByTestId("bottom-panel");
  await expect(panel).toBeVisible();
  // 任务流空态可见
  await expect(page.getByTestId("bottom-content-tasks")).toBeVisible();

  // <40% 视口高（720 的 40% = 288；常规高 240）
  const box = await panel.boundingBox();
  expect(box!.height).toBeLessThan(288);

  // 切日志 tab（数据来自 /logs 桩）
  await page.getByTestId("bottom-tab-logs").click();
  await expect(page.getByTestId("bottom-content-logs")).toBeVisible();
  await expect(page.getByText("面板验收日志行")).toBeVisible();

  // 跟随按钮状态可切换
  const follow = page.getByTestId("bottom-follow");
  await expect(follow).toHaveAttribute("aria-pressed", "true");
  await follow.click();
  await expect(follow).toHaveAttribute("aria-pressed", "false");

  // 最大化 > 常规高度，再还原
  await page.getByTestId("bottom-maximize").click();
  const maxBox = await panel.boundingBox();
  expect(maxBox!.height).toBeGreaterThan(box!.height);
  await page.getByTestId("bottom-maximize").click();

  // Cmd+J 关闭
  await page.keyboard.press(bottomHotkey);
  await expect(page.getByTestId("bottom-panel")).toHaveCount(0);
});

test("context dock toggles on the book page and coexists with the bottom panel", async ({ page }) => {
  await page.goto("/#/book/b1");

  // 书籍页 dock 沿袭默认可见（旧 BookSidebar 行为）
  const dock = page.getByTestId("context-dock");
  await expect(dock).toBeVisible({ timeout: 15_000 });

  // Cmd+Shift+D 关闭 → 再开
  await page.keyboard.press(dockHotkey);
  await expect(dock).toHaveCount(0);
  await page.keyboard.press(dockHotkey);
  await expect(dock).toBeVisible();

  // 与底部面板并存：面板仍在且高度不越界（编辑区不被挤压）
  await page.keyboard.press(bottomHotkey);
  await expect(dock).toBeVisible();
  const panelBox = await page.getByTestId("bottom-panel").boundingBox();
  expect(panelBox!.height).toBeLessThan(288);

  // 按页记忆：去首页（底栏未开过 → 收起），再回书籍页 → dock 状态从
  // localStorage 恢复为开
  await page.getByTestId("breadcrumb").getByRole("button", { name: "首页" }).click();
  await expect(page.getByTestId("breadcrumb")).toContainText("首页", { timeout: 10_000 });
  await expect(page.getByTestId("bottom-panel")).toHaveCount(0); // 首页未开过底栏
  await page.goto("/#/book/b1");
  await expect(dock).toBeVisible({ timeout: 15_000 });
});
