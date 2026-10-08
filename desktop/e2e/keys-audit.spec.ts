import { test, expect, type Page } from "./fixtures/app";

/**
 * Keys view + Security audit view end-to-end coverage.
 *
 * Keys: list rendering, fingerprint copy/toast, the insecure-key alert for the
 * mode-644 RSA key, generate-key modal IPC, and deploy-key modal IPC.
 * Audit: score/grade stats, severity filtering, remediation copy, and the
 * fix_permissions flow.
 */

async function grantClipboard(page: Page) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
}

test.describe("Keys", () => {
  test("lists both mock keys with fingerprints and permissions", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Keys");

    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Keys");
    await expect(page.getByText("id_ed25519", { exact: true })).toBeVisible();
    await expect(page.getByText("id_rsa_old", { exact: true })).toBeVisible();

    await expect(page.getByText("SHA256:Qx3n0b8kdl2bG9", { exact: true })).toBeVisible();
    await expect(page.getByText("SHA256:Zk1uB2mvLx77qa", { exact: true })).toBeVisible();

    // Secure ED25519 key shows mode 600, insecure RSA key shows 644.
    await expect(page.getByText("600", { exact: true })).toBeVisible();
    await expect(page.getByText("644", { exact: true })).toBeVisible();
  });

  test("copy fingerprint writes to the clipboard and toasts", async ({ app, page }) => {
    await grantClipboard(page);
    await app.gotoApp();
    await app.nav("Keys");

    await page.getByRole("button", { name: "Copy fingerprint of id_ed25519" }).click();

    await expect(page.getByText("Copied fingerprint", { exact: true })).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => navigator.clipboard.readText()))
      .toBe("SHA256:Qx3n0b8kdl2bG9");
  });

  test("shows the insecure-key alert and fix command for the 644 key", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Keys");

    const alert = page.getByRole("alert");
    await expect(alert).toBeVisible();
    await expect(alert).toContainText("1 private key is readable by other users.");
    await expect(alert.getByText("chmod 600 ~/.ssh/id_rsa_old")).toBeVisible();
  });

  test("generate-key modal submits generate_ssh_key with name and key type", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Keys");

    await page.getByRole("button", { name: "Generate key", exact: true }).first().click();
    const dialog = page.getByRole("dialog", { name: "Generate key" });
    await expect(dialog).toBeVisible();

    await dialog.getByLabel("File name").fill("id_ci_rsa");
    await dialog.getByRole("radio", { name: /RSA 4096/ }).click();
    await expect(dialog.getByRole("radio", { name: /RSA 4096/ })).toHaveAttribute("aria-checked", "true");

    await dialog.getByRole("button", { name: "Generate key", exact: true }).click();

    await app.waitForIpc("generate_ssh_key");
    const calls = await app.ipcCalls("generate_ssh_key");
    expect(calls).toEqual([{ name: "id_ci_rsa", keyType: "rsa" }]);

    await expect(page.getByText("Generated id_ci_rsa", { exact: true })).toBeVisible();
    await expect(dialog).toBeHidden();
    // The list reloads after generation, so the new key appears (the mock
    // echoes the name as both key name and comment).
    await expect(page.getByText("id_ci_rsa", { exact: true }).first()).toBeVisible();
  });

  test("deploy modal submits copy_ssh_key_to_target and shows the result", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Keys");

    const row = page.locator("tr").filter({ hasText: "id_ed25519" });
    await row.getByRole("button", { name: "Deploy" }).click();

    const dialog = page.getByRole("dialog", { name: "Deploy key" });
    await expect(dialog).toBeVisible();

    await dialog.getByRole("combobox").click();
    await page.getByRole("option", { name: /prod-api-01/ }).click();

    await dialog.getByRole("button", { name: "Deploy key", exact: true }).click();

    await app.waitForIpc("copy_ssh_key_to_target");
    const calls = await app.ipcCalls("copy_ssh_key_to_target");
    expect(calls).toEqual([{ target: "deploy@10.0.4.12", keyPath: "/home/me/.ssh/id_ed25519.pub" }]);

    await expect(page.getByText("Public key copied to deploy@10.0.4.12", { exact: true })).toBeVisible();
    await expect(dialog).toBeHidden();
  });
});

test.describe("Audit", () => {
  test("renders score, grade and severity stats", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Audit");

    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Security audit");
    await expect(page.getByText("78/100", { exact: true })).toBeVisible();
    await expect(page.getByText(/Grade\s+B\s+·\s+Good/)).toBeVisible();

    await expect(page.getByRole("button", { name: /^Critical/ })).toContainText("1");
    await expect(page.getByRole("button", { name: /^Warnings/ })).toContainText("1");
    await expect(page.getByRole("button", { name: /^Info/ })).toContainText("1");

    await expect(page.getByRole("heading", { name: "Private key world-readable" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "RSA-2048 key in use" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Agent forwarding disabled" })).toBeVisible();
  });

  test("severity filter narrows the findings", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Audit");

    await expect(page.getByRole("heading", { name: "Agent forwarding disabled" })).toBeVisible();

    await page.getByRole("button", { name: /^Critical/ }).click();
    await expect(page.getByRole("button", { name: /^Critical/ })).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByRole("heading", { name: "Private key world-readable" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "RSA-2048 key in use" })).toHaveCount(0);
    await expect(page.getByRole("heading", { name: "Agent forwarding disabled" })).toHaveCount(0);

    await page.getByRole("button", { name: /^Info/ }).click();
    await expect(page.getByRole("heading", { name: "Agent forwarding disabled" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Private key world-readable" })).toHaveCount(0);

    await page.getByRole("button", { name: /^All/ }).click();
    await expect(page.getByRole("heading", { name: "Private key world-readable" })).toBeVisible();
    await expect(page.getByRole("heading", { name: "RSA-2048 key in use" })).toBeVisible();
  });

  test("copy remediation writes the command to the clipboard and toasts", async ({ app, page }) => {
    await grantClipboard(page);
    await app.gotoApp();
    await app.nav("Audit");

    const finding = page.locator("li").filter({ has: page.getByRole("heading", { name: "Private key world-readable" }) });
    await expect(finding).toBeVisible();
    await finding.getByRole("button", { name: "Copy command" }).click();

    await expect(page.getByText("Copied command", { exact: true })).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => navigator.clipboard.readText()))
      .toBe("chmod 600 ~/.ssh/id_rsa_old");
  });

  test("fix permissions calls the backend and reports the result", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Audit");

    await expect(page.getByText("78/100", { exact: true })).toBeVisible();

    await page.getByRole("button", { name: "Fix permissions" }).click();

    await app.waitForIpc("fix_security_permissions");
    const calls = await app.ipcCalls("fix_security_permissions");
    expect(calls.length).toBe(1);
    await expect(page.getByText("Fixed permissions on 1 file", { exact: true })).toBeVisible();

    // The audit is re-run after repairing, proving the view refreshed.
    await app.waitForIpc("run_security_audit", 2);
  });
});
