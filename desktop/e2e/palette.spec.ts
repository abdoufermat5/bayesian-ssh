import { test, expect } from "./fixtures/app";

test.describe("command palette", () => {
  test("shows grouped results and closes with Escape", async ({ app, page }) => {
    await app.gotoApp();
    await app.openPalette();
    const dialog = page.getByRole("dialog", { name: "Command palette" });
    await expect(dialog.getByRole("group", { name: "Hosts" })).toBeVisible();
    await expect(dialog.getByRole("group", { name: "Actions" })).toBeVisible();
    await expect(dialog.getByRole("group", { name: "Navigation" })).toBeVisible();
    await expect(dialog.getByRole("option", { name: /prod-api-01/ })).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  });

  test("filters results as you type and shows the no-results state", async ({ app, page }) => {
    await app.gotoApp();
    await app.openPalette();
    const dialog = page.getByRole("dialog", { name: "Command palette" });
    const input = dialog.getByRole("combobox", { name: "Search commands" });

    await input.fill("keys");
    await expect(dialog.getByRole("option", { name: /Go to SSH keys/ })).toBeVisible();
    await expect(dialog.getByRole("option", { name: /prod-api-01/ })).toHaveCount(0);

    await input.fill("zzz-nothing");
    await expect(dialog.getByText(/No results for/)).toBeVisible();
  });

  test("arrow keys move the active option and Enter connects to a host", async ({ app, page }) => {
    await app.gotoApp();
    await app.openPalette();
    const dialog = page.getByRole("dialog", { name: "Command palette" });
    const first = dialog.getByRole("option").first();
    await expect(first).toHaveAttribute("aria-selected", "true");

    await page.keyboard.press("ArrowDown");
    const second = dialog.getByRole("option").nth(1);
    await expect(second).toHaveAttribute("aria-selected", "true");
    await expect(first).toHaveAttribute("aria-selected", "false");

    // Move back to the first host and connect.
    await page.keyboard.press("ArrowUp");
    await expect(first).toHaveAttribute("aria-selected", "true");
    const hostName = (await first.locator("span").first().textContent())!.trim();
    await page.keyboard.press("Enter");

    await expect(dialog).toBeHidden();
    await expect(page.locator(".workspace h1")).toHaveText("Terminals");
    await expect.poll(async () => app.ipcCalls("spawn_pty")).toEqual([
      expect.objectContaining({ connectionName: hostName }),
    ]);
  });

  test("navigation commands switch views", async ({ app, page }) => {
    await app.gotoApp();
    await app.openPalette();
    const dialog = page.getByRole("dialog", { name: "Command palette" });
    await dialog.getByRole("combobox", { name: "Search commands" }).fill("Go to settings");
    await page.keyboard.press("Enter");
    await expect(dialog).toBeHidden();
    await expect(page.locator(".workspace h1")).toHaveText("Settings");
  });

  test("theme commands are offered and apply a theme", async ({ app, page }) => {
    await app.gotoApp();
    await app.openPalette();
    const dialog = page.getByRole("dialog", { name: "Command palette" });
    await dialog.getByRole("combobox", { name: "Search commands" }).fill("Theme: OLED");
    await page.keyboard.press("Enter");
    await expect(page.locator("html")).toHaveClass(/theme-oled/);
  });
});
