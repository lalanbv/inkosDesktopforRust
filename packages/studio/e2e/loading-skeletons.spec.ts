import { test, expect, type Route } from "@playwright/test";

// P1-1/2/3/4 验收：三级加载体系（启动壳骨架 / 列表骨架 / 空态互斥）与首启
// 语言选择 Dialog。用 page.route + Promise 闸门控制响应时机（而非固定延时），
// 消除断言与骨架窗口的时序竞态；不改动服务端与 test-project 数据。

const PROJECT_READY = { language: "zh", languageExplicit: true };

/** 挂起的响应：断言完成后调用 release() 放行。 */
class GatedResponse {
  private releaseFn: (() => void) | null = null;
  private readonly gate: Promise<void>;

  constructor() {
    this.gate = new Promise((resolve) => {
      this.releaseFn = resolve;
    });
  }

  release() {
    this.releaseFn?.();
  }

  async handle(route: Route, body: Record<string, unknown>) {
    await this.gate;
    await route.fulfill({ json: body });
  }
}

test("startup renders the shell skeleton instead of a full-screen spinner", async ({ page }) => {
  const project = new GatedResponse();
  await page.route("**/api/v1/project", (route) => project.handle(route, PROJECT_READY));

  await page.goto("/#/");

  // 启动门 loading：整壳骨架（顶栏+侧栏+主区），不得再出现整屏 spinner。
  // /project 被闸门挂起，骨架态是确定性的，不受加载速度影响。
  const shell = page.locator('[data-loading="shell"]');
  await expect(shell).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("body .animate-spin")).toHaveCount(0);
  await expect(shell.locator('[data-slot="skeleton-sidebar"]')).toBeVisible();
  await expect(shell.locator('[data-slot="skeleton-header"]')).toBeVisible();
  await expect(shell.locator('[data-slot="skeleton-main"]')).toBeVisible();

  // 放行配置：骨架让位于真实布局
  project.release();
  await expect(shell).toHaveCount(0, { timeout: 10_000 });
  await expect(page.locator("aside")).toBeVisible({ timeout: 10_000 });
});

test("sidebar shows row skeletons while books load, then flips to the empty state", async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  const books = new GatedResponse();
  await page.route("**/api/v1/books", (route) => books.handle(route, { books: [] }));

  await page.goto("/#/");

  // 未就绪（闸门关闭）：行骨架可见，且不得误报空态
  const skeleton = page.locator('[data-loading="skeleton"]');
  await expect(skeleton.first()).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText("还没有书")).toHaveCount(0);

  // 就绪且空：骨架清空，空态出现
  books.release();
  await expect(skeleton).toHaveCount(0, { timeout: 10_000 });
  await expect(page.getByText("还没有书").first()).toBeVisible({ timeout: 10_000 });
});

test("dashboard renders three card skeletons while the library loads", async ({ page }) => {
  await page.route("**/api/v1/project", (route) => route.fulfill({ json: PROJECT_READY }));
  const books = new GatedResponse();
  await page.route("**/api/v1/books", (route) => books.handle(route, { books: [] }));

  await page.goto("/#/");

  const cards = page.locator('[data-slot="skeleton-cards"]');
  await expect(cards).toBeVisible({ timeout: 15_000 });
  await expect(cards.locator('[data-slot="skeleton-card"]')).toHaveCount(3);

  books.release();
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
