import { test, expect } from "./fixtures/app";

const VIEWS: Array<{ nav: string; title: string }> = [
  { nav: "Hosts", title: "Hosts" },
  { nav: "Terminals", title: "Terminals" },
  { nav: "Files", title: "Files" },
  { nav: "Tunnels", title: "Tunnels" },
  { nav: "Keys", title: "Keys" },
  { nav: "Audit", title: "Security audit" },
  { nav: "History", title: "History" },
  { nav: "Settings", title: "Settings" },
];

test.describe("app shell", () => {
  test("every sidebar view opens with exactly one h1 title", async ({ app, page }) => {
    await app.gotoApp();
    for (const view of VIEWS) {
      await app.nav(view.nav);
      const heading = page.locator(".workspace h1");
      await expect(heading, `${view.nav} should show one h1`).toHaveCount(1);
      await expect(heading).toHaveText(view.title);
    }
  });

  test("the active sidebar item is marked current", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Keys");
    await expect(page.locator('.sidebar [aria-label="Keys"]')).toHaveAttribute("aria-current", "page");
    await app.nav("Hosts");
    await expect(page.locator('.sidebar [aria-label="Hosts"]')).toHaveAttribute("aria-current", "page");
    await expect(page.locator('.sidebar [aria-label="Keys"]')).not.toHaveAttribute("aria-current", "page");
  });

  test("sidebar collapses and expands", async ({ app, page }) => {
    await app.gotoApp();
    const sidebar = page.locator(".sidebar");
    const expanded = (await sidebar.boundingBox())!.width;

    await page.getByRole("button", { name: "Collapse sidebar" }).click();
    await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();
    await expect(sidebar.getByText("Hosts", { exact: true })).toHaveCount(0);
    await expect.poll(async () => (await sidebar.boundingBox())!.width).toBeLessThan(expanded);

    await page.getByRole("button", { name: "Expand sidebar" }).click();
    await expect(page.getByRole("button", { name: "Collapse sidebar" })).toBeVisible();
    await expect(sidebar.getByText("Hosts", { exact: true })).toBeVisible();
  });

  test("profile switcher lists profiles and switching calls set_active_env", async ({ app, page }) => {
    await app.gotoApp();
    await page.locator('.sidebar [aria-haspopup="menu"]').click();
    const menu = page.getByRole("menu");
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitemradio", { name: /default/ })).toHaveAttribute("aria-checked", "true");
    await expect(menu.getByRole("menuitemradio", { name: /work/ })).toBeVisible();

    await menu.getByRole("menuitemradio", { name: /work/ }).click();
    await expect.poll(async () => app.ipcCalls("set_active_env")).toEqual([{ name: "work" }]);
    await expect(menu).toBeHidden();
    await expect(page.locator('.sidebar [aria-haspopup="menu"]')).toContainText("work");
  });

  test("'Manage profiles…' opens the profiles modal", async ({ app, page }) => {
    await app.gotoApp();
    await page.locator('.sidebar [aria-haspopup="menu"]').click();
    await page.getByRole("menuitem", { name: "Manage profiles…" }).click();
    await expect(page.getByRole("dialog", { name: /profile/i })).toBeVisible();
  });

  test("titlebar search opens the command palette", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "Open command palette" }).click();
    await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  });

  test("help and about buttons open their modals", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "Keyboard shortcuts" }).click();
    await expect(page.getByRole("dialog").filter({ hasText: "Keyboard shortcuts" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog").filter({ hasText: "Keyboard shortcuts" })).toBeHidden();

    await page.getByRole("button", { name: "About Bayesian SSH" }).click();
    await expect(page.getByRole("dialog", { name: "About Bayesian SSH" })).toBeVisible();
  });
});
