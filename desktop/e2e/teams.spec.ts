import { test, expect } from "./fixtures/app";

/**
 * Teams view: setup, unlock, the fingerprint-guarded grant, and invites.
 * The `teams_*` commands are stubbed per test; overrides must be
 * self-contained (they're stringified into the page).
 */

test.describe("Teams setup", () => {
  test.use({
    mockOptions: {
      overrides: {
        teams_status: () => ({ identity: null, unlocked: false, fingerprint: null, teams: [] }),
        teams_init: () => ({
          identity: { email: "ana@example.com", server_url: "https://teams.example.com", device_id: "d-2" },
          unlocked: true,
          fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB",
          teams: [],
        }),
      },
    },
  });

  test("sets up this device and shows its fingerprint", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await expect(page.getByRole("heading", { name: "Share connections with your team" })).toBeVisible();

    const setUp = page.getByRole("button", { name: "Set up this device" });
    await expect(setUp).toBeDisabled();

    await page.getByLabel("Sync server").fill("https://teams.example.com");
    await page.getByLabel("Your email").fill("ana@example.com");
    await page.getByLabel(/^Passphrase/).fill("correct horse");
    await page.getByLabel("Repeat passphrase").fill("correct hors");
    await expect(setUp).toBeDisabled();
    await page.getByLabel("Repeat passphrase").fill("correct horse");
    await setUp.click();

    await expect(page.getByText("SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB")).toBeVisible();
    const calls = (await app.ipcCalls("teams_init")) as Array<Record<string, unknown>>;
    expect(calls[0]).toMatchObject({
      server: "https://teams.example.com",
      email: "ana@example.com",
      passphrase: "correct horse",
    });
  });
});

test.describe("Teams unlock", () => {
  test.use({
    mockOptions: {
      overrides: {
        teams_status: () => ({
          identity: { email: "admin@example.com", server_url: "https://teams.example.com", device_id: "d-1" },
          unlocked: false,
          fingerprint: null,
          teams: [],
        }),
        teams_unlock: () => ({
          identity: { email: "admin@example.com", server_url: "https://teams.example.com", device_id: "d-1" },
          unlocked: true,
          fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
          teams: [],
        }),
      },
    },
  });

  test("unlocks with the passphrase", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await expect(page.getByRole("heading", { name: "Unlock Teams" })).toBeVisible();
    await page.getByPlaceholder("Teams passphrase").fill("correct horse");
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("heading", { name: "Create a team" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Lock" })).toBeVisible();
  });
});

test.describe("Teams admin", () => {
  test.use({
    mockOptions: {
      overrides: {
        teams_status: () => ({
          identity: { email: "admin@example.com", server_url: "https://teams.example.com", device_id: "d-1" },
          unlocked: true,
          fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
          teams: [{ team_id: "t-1", source_id: "s-1", name: "platform", hosts: 2, last_sync_at: null }],
        }),
        teams_members: () => [
          {
            email: "admin@example.com",
            role: "admin",
            devices: [{ name: "laptop", fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA" }],
          },
        ],
        teams_pending: () => [
          {
            email: "ana@example.com",
            device: { name: "ana-laptop", fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB" },
          },
        ],
        teams_grant: (args) => ({
          email: "ana@example.com",
          device: { name: "ana-laptop", fingerprint: (args as { fingerprint: string }).fingerprint },
        }),
        teams_invite: () => ({
          email: "ana@example.com",
          expires_at: "2030-01-08T00:00:00Z",
          token: "one-time-token-123",
          admin_fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
          server_url: "https://teams.example.com",
        }),
      },
    },
  });

  test("grants the key only when the received fingerprint matches", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: /platform/ }).click();
    await page.getByRole("button", { name: "Check" }).click();
    await expect(page.getByText("ana@example.com · ana-laptop")).toBeVisible();

    const grant = page.getByRole("button", { name: "Grant key" });
    const input = page.getByPlaceholder("Paste the fingerprint ana@example.com sent you");
    await expect(grant).toBeDisabled();

    await input.fill("SHA256:somethingElseEntirelyCCCCCCCCCCCCCCCCCCCC");
    await expect(grant).toBeDisabled();
    await expect(page.getByText("Doesn't match this device. Don't grant it.")).toBeVisible();

    await input.fill("SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB");
    await expect(grant).toBeEnabled();
    await grant.click();

    await expect(page.getByText("Granted the platform key to ana@example.com")).toBeVisible();
    const calls = (await app.ipcCalls("teams_grant")) as Array<Record<string, unknown>>;
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({
      team: "t-1",
      fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB",
    });
  });

  test("invite shows the commands to send", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: /platform/ }).click();
    await page.getByPlaceholder("ana@example.com").fill("ana@example.com");
    await page.getByRole("button", { name: "Invite", exact: true }).click();
    await expect(
      page.getByText(
        "bssh teams join one-time-token-123 --fingerprint SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
      ),
    ).toBeVisible();
  });
});
