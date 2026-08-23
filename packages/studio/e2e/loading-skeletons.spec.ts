import { test, expect } from "@playwright/test";

// P1-1/2/3/4 验收：三级加载体系（启动壳骨架 / 列表骨架 / 空态互斥）与首启
// 语言选择 Dialog。全部用 page.route 在浏览器侧制造延迟与响应，不改动
// 服务端与 test-project 数据。

const PROJECT_READY = { language: "zh", languageExplicit: true };

test("startup renders the shell skeleton instead of a full-screen spinner", async ({ page }) => {
  await page.route("**/api/v1/project", async (route) => {
    // 3s：dev 模式首帧含 vite 冷转换，窗口太窄断言可能错过骨架帧
    await page.waitForTimeout(3000);
    await route.fulfill({ json: PROJECT_READY });
  });

  await page.goto("/#/");

  // 启动门 loading：整壳骨架（顶栏+侧栏+主区），不得再出现整屏 spinner
  const shell = page.locator('[data-loading="shell"]');
  await expect(shell).toBeVisible();
  await expect(page.locator("body .animate-spin")).toHaveCount(0);
  await expect(shell.locator('[data-slot="skeleton-sidebar"]')).toBeVisible();
  await expect(shell.locator('[data-slot="skeleton-header"]')).toBeVisible();
  await expect(shell.locator('[data-slot="skeleton-main"]')).toBeVisible();

  // 配置到达后骨架让位于真实布局
  await expect(shell).toHaveCount(0, { timeout: 10_000 });
  await expect(page.locator("aside")).toBeVisible({ timeout: 10_000 });
});

test("sidebar shows row skeletons while books load, then flips to the empty state", async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", async (route) => {
    // 3s：骨架有 200ms 延迟出现逻辑，窗口太窄会让断言错过骨架帧
    await page.waitForTimeout(3000);
    await route.fulfill({ json: { books: [] } });
  });

  await page.goto("/#/");

  // 未就绪：行骨架可见（>200ms 延迟出现），且不得误报空态
  const skeleton = page.locator('[data-loading="skeleton"]');
  await expect(skeleton.first()).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText("还没有书")).toHaveCount(0);

  // 就绪且空：骨架清空，空态出现（侧栏与仪表各一处）
  await expect(skeleton).toHaveCount(0, { timeout: 10_000 });
  await expect(page.getByText("还没有书").first()).toBeVisible({ timeout: 10_000 });
});

test("dashboard renders three card skeletons while the library loads", async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  await page.route("**/api/v1/books", async (route) => {
    await page.waitForTimeout(3000);
    await route.fulfill({ json: { books: [] } });
  });

  await page.goto("/#/");

  const cards = page.locator('[data-slot="skeleton-cards"]');
  await expect(cards).toBeVisible({ timeout: 10_000 });
  await expect(cards.locator('[data-slot="skeleton-card"]')).toHaveCount(3);
  await expect(cards).toHaveCount(0, { timeout: 10_000 });
});

test("first-launch language selector is a forced dialog above the ready shell", async ({ page }) => {
  let languagePosted = false;
  await page.route("**/api/v1/project", (route) =>
    route.fulfill({
      json: { language: languagePosted ? "zh" : "en", languageExplicit: languagePosted },
    }),
  );
  await page.route("**/api/v1/project/language", async (route) => {
    languagePosted = true;
    await route.fulfill({ json: { ok: true } });
  });

  await page.goto("/#/");

  // 主布局就绪（侧栏可见），语言选择以 Dialog 覆盖其上，而非整屏替换
  const dialog = page.locator('[data-slot="language-selector"]');
  await expect(dialog).toBeVisible({ timeout: 10_000 });
  await expect(page.locator("aside")).toBeVisible({ timeout: 10_000 });

  // 强制选择：ESC 不关闭
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();

  await dialog.locator('[data-language="zh"]').click();
  await expect(dialog).toHaveCount(0, { timeout: 10_000 });
});
