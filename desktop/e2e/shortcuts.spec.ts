import { test, expect } from "./fixtures/app";

test.describe("global shortcuts", () => {
  test("number keys 1-6 switch views", async ({ app, page }) => {
    await app.gotoApp();
    const expectations: Array<[string, string]> = [
      ["1", "Hosts"],
      ["2", "Terminals"],
      ["3", "Keys"],
      ["4", "Security audit"],
      ["5", "History"],
      ["6", "Settings"],
    ];
    for (const [key, title] of expectations) {
      await page.keyboard.press(key);
      await expect(page.locator(".workspace h1")).toHaveText(title);
    }
  });

  test("'/' focuses the hosts filter", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("/");
    await expect(page.locator(".search-input")).toBeFocused();
  });

  test("'n' and Ctrl+N open the new host modal", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toBeHidden();

    await page.keyboard.press("Control+n");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toBeVisible();
  });

  test("Ctrl+K opens the command palette and '?' opens help", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("Control+k");
    await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog", { name: "Command palette" })).toBeHidden();

    await page.keyboard.press("?");
    await expect(page.getByRole("dialog").filter({ hasText: "Keyboard shortcuts" })).toBeVisible();
  });

  test("Escape closes an open modal", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");
    const dialog = page.getByRole("dialog").filter({ hasText: "New host" });
    await expect(dialog).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  });

  test("shortcuts do not fire while typing in an input", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("/");
    const input = page.locator(".search-input");
    await expect(input).toBeFocused();

    await input.type("12n?");
    await expect(page.locator(".workspace h1")).toHaveText("Hosts");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toHaveCount(0);
    await expect(page.getByRole("dialog").filter({ hasText: "Keyboard shortcuts" })).toHaveCount(0);
    await expect(input).toHaveValue("12n?");

    // Ctrl+K is intentionally global even from an input.
    await page.keyboard.press("Control+k");
    await expect(page.getByRole("dialog", { name: "Command palette" })).toBeVisible();
  });

  test("view shortcuts are ignored while a modal is open", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toBeVisible();
    await page.keyboard.press("5");
    await expect(page.locator(".workspace h1")).toHaveText("Hosts");
    await expect(page.getByRole("dialog").filter({ hasText: "New host" })).toBeVisible();
  });
});
