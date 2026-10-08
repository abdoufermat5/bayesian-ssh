import { test, expect, type AppFixture, type Page } from "./fixtures/app";

/**
 * Updates settings e2e coverage.
 *
 * The updater commands are stubbed by the IPC mock: `update_managed_by`
 * defaults to null and `check_update` to null (up to date). Tests override
 * them through `mockOptions.overrides` to exercise each channel.
 */

const SECTIONS = 'nav[aria-label="Settings sections"]';

async function openUpdates(app: AppFixture, page: Page) {
  await app.gotoApp();
  await app.nav("Settings");
  await page.locator(SECTIONS).getByRole("button", { name: "Updates", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Updates", level: 2 })).toBeVisible();
}

const UPDATE_VERSION = "2.6.0";
const UPDATE_NOTES = "Fixes and improvements.";

test.describe("updates", () => {
  test("up to date reports the latest version", async ({ app, page }) => {
    await openUpdates(app, page);

    await page.getByRole("button", { name: "Check for updates" }).click();

    await expect(page.getByText("You're on the latest version.")).toBeVisible();
    await expect.poll(async () => app.ipcCalls("check_update")).toHaveLength(1);
  });

  test.describe("update available", () => {
    // Override bodies are stringified into the browser, so they must be
    // self-contained: no closing over this module's scope.
    test.use({
      mockOptions: {
        overrides: {
          check_update: () => ({
            version: "2.6.0",
            currentVersion: "2.4.0",
            notes: "Fixes and improvements.",
          }),
        },
      },
    });

    test("shows the version and notes, then install confirms and invokes install_update", async ({
      app,
      page,
    }) => {
      await openUpdates(app, page);

      await page.getByRole("button", { name: "Check for updates" }).click();

      await expect(page.getByText(`Update available: ${UPDATE_VERSION}`)).toBeVisible();
      // Release notes render as plain preformatted text.
      await expect(page.locator("pre.code-block")).toHaveText(UPDATE_NOTES);

      // Installing first asks for confirmation.
      await page.getByRole("button", { name: "Install and restart" }).click();
      const dialog = page.getByRole("dialog", { name: "Install update" });
      await expect(dialog).toBeVisible();
      await expect(page.getByRole("heading", { name: `Install Bayesian SSH ${UPDATE_VERSION}?` })).toBeVisible();
      expect(await app.ipcCalls("install_update")).toHaveLength(0);

      await dialog.getByRole("button", { name: "Install and restart" }).click();

      await expect
        .poll(async () => app.ipcCalls("install_update"))
        .toEqual([{ version: UPDATE_VERSION }]);
      await expect(dialog).toBeHidden();
    });
  });

  test.describe("install with active sessions", () => {
    test.use({
      mockOptions: {
        overrides: {
          check_update: () => ({
            version: "2.6.0",
            currentVersion: "2.4.0",
            notes: null,
          }),
          count_active_sessions: () => 2,
        },
      },
    });

    test("confirmation warns that open sessions will be closed", async ({ app, page }) => {
      await openUpdates(app, page);
      await page.getByRole("button", { name: "Check for updates" }).click();
      await page.getByRole("button", { name: "Install and restart" }).click();

      const dialog = page.getByRole("dialog", { name: "Install update" });
      await expect(dialog).toBeVisible();
      await expect(dialog.getByText("2 sessions")).toBeVisible();
      await expect(dialog.getByText("will be closed.")).toBeVisible();
    });
  });

  test.describe("snap channel", () => {
    test.use({ mockOptions: { overrides: { update_managed_by: () => "snap" } } });

    test("hides the check button and points at the Snap Store", async ({ app, page }) => {
      await openUpdates(app, page);

      await expect(page.getByText("Managed by the Snap Store")).toBeVisible();
      await expect(page.locator("code.system-value")).toHaveText("sudo snap refresh bayesian-ssh");
      await expect(page.getByRole("button", { name: "Check for updates" })).toHaveCount(0);
    });
  });

  test.describe("manual channel", () => {
    test.use({ mockOptions: { overrides: { update_managed_by: () => "manual" } } });

    test("hides the check button and explains manual updates", async ({ app, page }) => {
      await openUpdates(app, page);

      await expect(page.getByText("Updated manually")).toBeVisible();
      await expect(page.getByRole("button", { name: "Check for updates" })).toHaveCount(0);
    });
  });
});
