import { test, expect } from "./fixtures/app";
import type { AppFixture, Page } from "./fixtures/app";

/**
 * Tunnels view end-to-end coverage.
 *
 * Tunnel rules have no backend IPC and live only for the current view session,
 * so these tests exercise the empty state, modal validation, table rows, the
 * enabled/disabled switch, editing and deletion. Expectations target
 * user-visible behaviour and toasts.
 */

async function openTunnels(app: AppFixture, page: Page) {
  await app.gotoApp();
  await app.nav("Tunnels");
  await expect(page.getByRole("heading", { name: "Tunnels" })).toBeVisible();
}

/** Create a local tunnel through the modal, defaulting to the first host. */
async function createTunnel(page: Page, port: number) {
  await page.getByRole("button", { name: "New tunnel" }).first().click();
  const dialog = page.getByRole("dialog", { name: "New tunnel" });
  await expect(dialog).toBeVisible();
  await dialog.locator("#local-port-input").fill(String(port));
  await dialog.getByRole("button", { name: "Create tunnel" }).click();
  await expect(dialog).toBeHidden();
}

test.describe("tunnels view", () => {
  test("renders the empty state initially", async ({ app, page }) => {
    await openTunnels(app, page);

    await expect(page.locator(".empty-state-title")).toHaveText("No tunnels yet");
    await expect(page.getByRole("group", { name: "Tunnel presets" })).toBeVisible();
    await expect(page.getByRole("button", { name: "New tunnel" }).first()).toBeEnabled();
    await expect(page.getByRole("table")).toHaveCount(0);
  });

  test("a preset pre-fills the form and port validation gates submission", async ({ app, page }) => {
    await openTunnels(app, page);

    await page
      .getByRole("group", { name: "Tunnel presets" })
      .getByRole("button", { name: /^Web/ })
      .click();

    const dialog = page.getByRole("dialog", { name: "New tunnel" });
    await expect(dialog).toBeVisible();

    // Preset values land in the form and the SSH preview.
    await expect(dialog.locator("#local-port-input")).toHaveValue("8080");
    await expect(dialog.locator("#remote-port-input")).toHaveValue("80");
    await expect(dialog).toContainText("ssh -N -L 8080:localhost:80");

    // Out-of-range port is rejected.
    await dialog.locator("#local-port-input").fill("70000");
    await expect(dialog.getByText("Enter a port between 1 and 65535.")).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Create tunnel" })).toBeDisabled();

    // A valid port clears the error and enables submission.
    await dialog.locator("#local-port-input").fill("9090");
    await expect(dialog.getByText("Enter a port between 1 and 65535.")).toHaveCount(0);
    await expect(dialog.getByRole("button", { name: "Create tunnel" })).toBeEnabled();
    await expect(dialog).toContainText("ssh -N -L 9090:localhost:80");
  });

  test("creating a tunnel adds a row", async ({ app, page }) => {
    await openTunnels(app, page);
    await createTunnel(page, 8123);

    const row = page.locator("tbody tr");
    await expect(row).toHaveCount(1);
    await expect(row).toContainText("prod-api-01");
    await expect(row).toContainText("Local");
    await expect(row).toContainText("127.0.0.1:8123");
    await expect(row).toContainText("localhost:80");
    await expect(page.getByText("1 of 1 enabled")).toBeVisible();
  });

  test("toggling a tunnel enables and disables it", async ({ app, page }) => {
    await openTunnels(app, page);
    await createTunnel(page, 8200);

    const toggle = page.getByRole("switch");
    await expect(toggle).toHaveAttribute("aria-checked", "true");
    await expect(toggle).toHaveText("Enabled");
    await expect(page.getByText("1 of 1 enabled")).toBeVisible();

    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-checked", "false");
    await expect(toggle).toHaveText("Disabled");
    await expect(page.getByText("0 of 1 enabled")).toBeVisible();

    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-checked", "true");
    await expect(page.getByText("1 of 1 enabled")).toBeVisible();
  });

  test("editing a tunnel updates the existing row", async ({ app, page }) => {
    await openTunnels(app, page);
    await createTunnel(page, 8300);

    await page.getByRole("button", { name: "Edit tunnel" }).click();
    const dialog = page.getByRole("dialog", { name: "Edit tunnel" });
    await expect(dialog).toBeVisible();
    await expect(dialog.locator("#local-port-input")).toHaveValue("8300");

    await dialog.locator("#local-port-input").fill("9300");
    await dialog.getByRole("button", { name: "Save changes" }).click();
    await expect(dialog).toBeHidden();

    await expect(page.locator("tbody tr")).toHaveCount(1);
    await expect(page.locator("tbody tr")).toContainText("127.0.0.1:9300");
    await expect(page.locator("tbody")).not.toContainText("8300");
  });

  test("deleting a tunnel removes it and restores the empty state", async ({ app, page }) => {
    await openTunnels(app, page);
    await createTunnel(page, 8400);
    await expect(page.locator("tbody tr")).toHaveCount(1);

    await page.getByRole("button", { name: "Delete tunnel" }).click();
    await expect(page.locator("tbody tr")).toHaveCount(0);
    await expect(page.locator(".empty-state-title")).toHaveText("No tunnels yet");
  });
});
