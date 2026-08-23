import { test, expect } from "@playwright/test";

// P3-3 验收：深链进入对应标签、预览语义（单击替换预览、双击驻留）、
// Cmd+N 切换、关闭位移。V1/V2 布局均适用（TabStrip 挂在主列）。

const PROJECT_READY = { language: "zh", languageExplicit: true };
const tabHotkey = (n: number) => (process.platform === "darwin" ? `Meta+${n}` : `Control+${n}`);

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", (route) =>
    route.fulfill({
      json: {
        books: [
          { id: "b1", title: "山河志" },
          { id: "b2", title: "夜航西飞" },
        ],
      },
    }),
  );
  await page.route("**/api/v1/interactive-films", (route) => route.fulfill({ json: { films: [] } }));
  await page.route("**/api/v1/daemon", (route) => route.fulfill({ json: { running: false } }));
});

test("deep link opens the hash route as a preview tab", async ({ page }) => {
  await page.goto("/#/book/b1");

  const strip = page.getByTestId("tab-strip");
  await expect(strip).toBeVisible({ timeout: 10_000 });
  await expect(strip.getByText("山河志")).toBeVisible();
  await expect(page.getByTestId("breadcrumb")).toContainText("山河志");
});

test("preview replaces on click, double-click stays, Cmd+2 switches, close shifts activation", async ({ page }) => {
  await page.goto("/#/");

  const strip = page.getByTestId("tab-strip");
  await expect(strip).toBeVisible({ timeout: 10_000 });
  // 首页深链 → 预览标签
  await expect(strip.getByText("首页")).toBeVisible();

  // 单击第一本书 → 预览标签被替换（首页让位）
  await page.locator("aside").getByRole("button", { name: "山河志", exact: true }).click();
  await expect(strip.getByText("山河志")).toBeVisible();
  await expect(strip.getByText("首页")).toHaveCount(0);

  // 单击第二本书 → 同一预览标签原位替换
  await page.locator("aside").getByRole("button", { name: "夜航西飞", exact: true }).click();
  await expect(strip.getByText("夜航西飞")).toBeVisible();
  await expect(strip.getByText("山河志")).toHaveCount(0);
  await expect(strip.locator('[data-testid^="tab-"]')).toHaveCount(1);

  // 双击标签 → 转正常驻（预览字样消失）
  await strip.getByText("夜航西飞").dblclick();
  await expect(strip.getByText("预览")).toHaveCount(0);

  // 再点第一本书 → 新预览标签，常驻标签保留（2 标签）
  await page.locator("aside").getByRole("button", { name: "山河志", exact: true }).click();
  await expect(strip.getByText("山河志")).toBeVisible();
  await expect(strip.getByText("夜航西飞")).toBeVisible();
  await expect(strip.locator('[data-testid^="tab-"]')).toHaveCount(2);

  // Cmd+1 切回第 1 个标签（夜航西飞为常驻；当前激活在第 2 个）
  await page.keyboard.press(tabHotkey(1));
  await expect(page.getByTestId("breadcrumb")).toContainText("夜航西飞", { timeout: 10_000 });

  // 关闭当前标签 → 激活位移到剩余标签
  const activeTab = strip.locator('[data-active="true"]');
  await activeTab.getByRole("button", { name: /关闭/ }).click();
  await expect(strip.getByText("夜航西飞")).toHaveCount(0);
  await expect(page.getByTestId("breadcrumb")).toContainText("山河志");
});
