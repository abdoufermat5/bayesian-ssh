import { test, expect } from "./fixtures/app";

const THEMES = [
  { card: "Graphite", cls: "theme-zinc", id: "zinc" },
  { card: "Midnight", cls: "theme-cyberpunk", id: "cyberpunk" },
  { card: "OLED black", cls: "theme-oled", id: "oled" },
  { card: "Slate", cls: "theme-slate", id: "slate" },
];

test.describe("themes", () => {
  for (const theme of THEMES) {
    test(`applying ${theme.card} sets the html class and the app stays healthy`, async ({ app, page }) => {
      await app.gotoApp();
      await app.nav("Settings");
      await page.getByRole("button", { name: "Appearance" }).click();
      await page.getByRole("radio", { name: theme.card }).click();

      await expect(page.locator("html")).toHaveClass(new RegExp(theme.cls));
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme.id);
      await expect(page.getByRole("radio", { name: theme.card })).toHaveAttribute("aria-checked", "true");
      await expect.poll(async () => app.ipcCalls("save_desktop_settings")).toEqual([
        expect.objectContaining({ settings: expect.objectContaining({ theme: theme.id }) }),
      ]);
      await app.expectNoPageErrors();
    });
  }

  test("the selected theme persists across a reload", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Settings");
    await page.getByRole("button", { name: "Appearance" }).click();
    await page.getByRole("radio", { name: "Midnight" }).click();
    await expect(page.locator("html")).toHaveClass(/theme-cyberpunk/);

    await page.reload();
    await expect(page.locator(".app-shell")).toBeVisible();
    await expect(page.locator("html")).toHaveClass(/theme-cyberpunk/);
  });
});
