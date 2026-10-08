import { test, expect } from "./fixtures/app";

/**
 * Global toasts (Toast.svelte + notifications.svelte.ts).
 *
 * A successful action and a forced IPC failure must each surface a toast with
 * the right ARIA role (status vs alert) carrying the exact message, and the
 * store's 3s timer must remove it again.
 */

test("success action shows a success toast that auto-dismisses", async ({ app, page }) => {
  await app.gotoApp();

  // Copying the SSH command for a host notifies success.
  await page.getByRole("button", { name: "Copy SSH command for prod-api-01" }).click();

  const toast = page.getByRole("status").filter({ hasText: "SSH command copied to clipboard" });
  await expect(toast).toBeVisible();

  // notifications store dismisses after DURATION_MS (3s) — expect timeout is 7s.
  await expect(toast).toHaveCount(0);
});

test.describe("failing action", () => {
  test.use({
    mockOptions: {
      overrides: {
        add_connection: () => {
          throw "boom";
        },
      },
    },
  });

  test("IPC error shows an error toast that auto-dismisses", async ({ app, page }) => {
    await app.gotoApp();

    await page.keyboard.press("n");
    const modal = page.getByRole("dialog", { name: "New host" });
    await expect(modal).toBeVisible();

    await page.locator("#c-name").fill("boom-host");
    await page.locator("#c-host").fill("boom.example.com");
    await modal.getByRole("button", { name: "Add host" }).click();

    // The rejection is recorded, then surfaced verbatim as an error toast.
    await app.waitForIpc("add_connection", 1);

    const toast = page.getByRole("alert").filter({ hasText: "boom" });
    await expect(toast).toBeVisible();

    await expect(toast).toHaveCount(0);
  });
});
