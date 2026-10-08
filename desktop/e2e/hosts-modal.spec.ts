import { test, expect, type Page } from "./fixtures/app";

/**
 * Hosts view — "New host" / "Edit host" connection modal.
 *
 * Covers: opening via the `N` shortcut and the view button, inline field
 * validation, the exact `add_connection` / `edit_connection` IPC payloads,
 * Enter-to-submit, Escape-to-close, and the empty-profile state.
 */

const dialog = (page: Page, name: string) => page.getByRole("dialog", { name });

test.describe("new host modal", () => {
  test("'N' shortcut opens the New host modal", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");

    await expect(dialog(page, "New host")).toBeVisible();
    await expect(page.locator("#c-name")).toBeFocused();
  });

  test("'New host' button opens the modal; empty submits show inline errors and never save", async ({
    app,
    page,
  }) => {
    await app.gotoApp();
    await page.locator(".view-header").getByRole("button", { name: "New host" }).click();

    const modal = dialog(page, "New host");
    await expect(modal).toBeVisible();

    // Submit with everything empty: both inline errors appear.
    await modal.getByRole("button", { name: "Add host" }).click();
    await expect(page.locator("#c-name-error")).toHaveText("Name is required");
    await expect(page.locator("#c-host-error")).toHaveText("Host is required");
    await expect(page.locator("#c-name")).toHaveAttribute("aria-invalid", "true");

    // Fill the name only and submit again: name error clears, host error stays.
    await page.locator("#c-name").fill("prod-api-9");
    await modal.getByRole("button", { name: "Add host" }).click();
    await expect(page.locator("#c-name-error")).toHaveCount(0);
    await expect(page.locator("#c-host-error")).toHaveText("Host is required");

    // Nothing was ever persisted.
    expect(await app.ipcCalls("add_connection")).toHaveLength(0);
  });

  test("valid submit records the exact add_connection payload", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");
    const modal = dialog(page, "New host");
    await expect(modal).toBeVisible();

    await page.locator("#c-name").fill("web-01");
    await page.locator("#c-host").fill("web01.example.com");
    await page.locator("#c-port").fill("2222");
    await page.locator("#c-user").fill("deploy");
    await page.locator("#c-tags").fill("prod, web");

    // Expand Advanced and configure a jump host + Kerberos.
    await modal.getByRole("button", { name: /Advanced/ }).click();
    await page.locator("#c-bastion").fill("jump.example.com");
    await page.locator("#c-bastion-user").fill("jumpuser");
    await page.locator("#c-krb").check();

    await modal.getByRole("button", { name: "Add host" }).click();

    await app.waitForIpc("add_connection", 1);
    const calls = await app.ipcCalls("add_connection");
    expect(calls).toHaveLength(1);
    expect(calls[0]).toEqual({
      name: "web-01",
      host: "web01.example.com",
      user: "deploy",
      port: 2222,
      kerberos: true,
      bastion: "jump.example.com",
      bastionUser: "jumpuser",
      keyPath: null,
      tags: ["prod", "web"],
    });

    // Saved => modal closes.
    await expect(modal).toHaveCount(0);
  });

  test("Enter submits the form", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");
    await expect(dialog(page, "New host")).toBeVisible();

    await page.locator("#c-name").fill("enter-host");
    await page.locator("#c-host").fill("enter.example.com");
    await page.locator("#c-name").press("Enter");

    await app.waitForIpc("add_connection", 1);
    const calls = await app.ipcCalls("add_connection");
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({
      name: "enter-host",
      host: "enter.example.com",
      tags: [],
    });
  });

  test("Escape closes the modal without saving", async ({ app, page }) => {
    await app.gotoApp();
    await page.keyboard.press("n");
    const modal = dialog(page, "New host");
    await expect(modal).toBeVisible();

    await page.locator("#c-name").fill("discarded");
    await page.keyboard.press("Escape");

    await expect(modal).toHaveCount(0);
    expect(await app.ipcCalls("add_connection")).toHaveLength(0);
  });
});

test.describe("edit host modal", () => {
  test("row action prefills fields and records edit_connection", async ({ app, page }) => {
    await app.gotoApp();

    await page.getByRole("button", { name: "Edit prod-api-01" }).click();
    const modal = dialog(page, "Edit host");
    await expect(modal).toBeVisible();

    // Pre-filled from the seeded connection.
    await expect(page.locator("#c-name")).toHaveValue("prod-api-01");
    await expect(page.locator("#c-host")).toHaveValue("10.0.4.12");
    await expect(page.locator("#c-user")).toHaveValue("deploy");
    await expect(page.locator("#c-port")).toHaveValue("22");
    await expect(page.locator("#c-tags")).toHaveValue("prod, api");
    // Advanced is already open because the host uses a jump host.
    await expect(page.locator("#c-bastion")).toHaveValue("bastion.corp");
    await expect(page.locator("#c-bastion-user")).toHaveValue("jump");

    await page.locator("#c-name").fill("prod-api-01-renamed");
    await modal.getByRole("button", { name: "Save changes" }).click();

    await app.waitForIpc("edit_connection", 1);
    const calls = await app.ipcCalls("edit_connection");
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({
      id: "id-0",
      name: "prod-api-01-renamed",
      host: "10.0.4.12",
      user: "deploy",
      port: 22,
      tags: ["prod", "api"],
    });

    await expect(modal).toHaveCount(0);
  });

  test("Ctrl+E opens the edit modal for the selected host", async ({ app, page }) => {
    await app.gotoApp();

    await page.locator("tr", { hasText: "staging-web" }).click();
    await page.keyboard.press("Control+e");

    const modal = dialog(page, "Edit host");
    await expect(modal).toBeVisible();
    await expect(page.locator("#c-name")).toHaveValue("staging-web");
    await expect(page.locator("#c-host")).toHaveValue("staging.example.com");
    await expect(page.locator("#c-port")).toHaveValue("2222");
  });
});

test.describe("empty profile", () => {
  test.use({ mockOptions: { overrides: { get_connections: () => [] } } });

  test("shows the empty state; Import calls import_ssh_config", async ({ app, page }) => {
    await app.gotoApp();

    await expect(page.getByText("No hosts yet")).toBeVisible();
    const empty = page.locator(".empty-state");
    await expect(empty.getByRole("button", { name: "New host" })).toBeVisible();
    await empty.getByRole("button", { name: /Import ~\/\.ssh\/config/ }).click();

    await app.waitForIpc("import_ssh_config", 1);
    expect(await app.ipcCalls("import_ssh_config")).toHaveLength(1);
  });
});
