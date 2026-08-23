import { test, expect } from "@playwright/test";

// P3-1 验收：活动栏四区布局（V2 双轨）。localStorage 预置开关启用 V2；
// 断言区切换、按区渲染、底部设置入口与导航可用性。V1 回归由其余存量用例覆盖。

const PROJECT_READY = { language: "zh", languageExplicit: true };

test.beforeEach(async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", (route) => route.fulfill({ json: { books: [] } }));
  await page.route("**/api/v1/interactive-films", (route) => route.fulfill({ json: { films: [] } }));
  await page.route("**/api/v1/daemon", (route) => route.fulfill({ json: { running: false } }));
  await page.addInitScript(() => {
    localStorage.setItem("inkos:studio:nav-layout-v2", "true");
  });
});

test("activity bar switches zones and the side panel renders per zone", async ({ page }) => {
  await page.goto("/#/");

  const bar = page.locator('[data-slot="activity-bar"]');
  await expect(bar).toBeVisible({ timeout: 10_000 });
  await expect(bar.locator('[data-testid="activity-create"]')).toBeVisible();
  await expect(bar.locator('[data-testid="activity-tools"]')).toBeVisible();
  await expect(bar.locator('[data-testid="activity-manage"]')).toBeVisible();
  await expect(bar.locator('[data-testid="activity-film"]')).toBeVisible();

  // 默认跟随路由（dashboard → 创作区）：面板显示开始创作，不显示工具组
  // （「翻译译介」在创作宫格也有创建项，负断言换工具区专属的「市场雷达」）
  await expect(bar.locator('[data-testid="activity-create"]')).toHaveAttribute("aria-current", "page");
  await expect(page.getByText("开始创作").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "市场雷达" })).toHaveCount(0);

  // 切到工具区：题材并入工具组，管理/创作内容隐藏
  await bar.locator('[data-testid="activity-tools"]').click();
  await expect(bar.locator('[data-testid="activity-tools"]')).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("button", { name: "题材" })).toBeVisible();
  await expect(page.getByRole("button", { name: "翻译译介" })).toBeVisible();
  await expect(page.getByText("开始创作", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "模型配置" })).toHaveCount(0);

  // 工具区点翻译译介 → 面包屑到达
  await page.getByRole("button", { name: "翻译译介" }).click();
  await expect(page.getByTestId("breadcrumb")).toContainText("翻译译介", { timeout: 10_000 });

  // 底部设置直达项目设置页
  await page.locator('[data-testid="activity-settings"]').click();
  await expect(page.getByTestId("breadcrumb")).toContainText("项目设置", { timeout: 10_000 });
});
