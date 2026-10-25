import { test, expect } from "./fixtures/app";

/**
 * Teams view: setup, unlock, join, the fingerprint-guarded grant, removal,
 * the audit log states and role-aware actions. The `teams_*` commands are
 * stubbed per test; overrides must be self-contained (they're stringified
 * into the page).
 */

const ADMIN_FP = "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA";
const ANA_FP = "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB";

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
        teams_attention: () => [],
      },
    },
  });

  test("explains why setup is disabled, then shows this device's fingerprint", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await expect(page.getByRole("heading", { name: "Share connections with your team" })).toBeVisible();
    await expect(page.getByText("nobody can reset it")).toBeVisible();

    const setUp = page.getByRole("button", { name: "Set up this device" });
    await expect(setUp).toBeDisabled();
    await expect(page.getByText("Enter the sync server")).toBeVisible();

    await page.getByLabel("Sync server").fill("https://teams.example.com");
    await page.getByLabel("Your email").fill("ana@example.com");
    await page.getByLabel("Passphrase", { exact: true }).fill("correct horse");
    await page.getByLabel("Repeat passphrase").fill("correct hors");
    await expect(setUp).toBeDisabled();
    await expect(page.getByText("The passphrases don't match")).toBeVisible();
    await page.getByLabel("Repeat passphrase").fill("correct horse");
    await setUp.click();

    await expect(page.locator("code.fingerprint").first()).toHaveText(ANA_FP);
    const calls = (await app.ipcCalls("teams_init")) as Array<Record<string, unknown>>;
    expect(calls[0]).toMatchObject({
      server: "https://teams.example.com",
      email: "ana@example.com",
      passphrase: "correct horse",
      // No invite pasted: nothing to show an invite-only server.
      inviteToken: null,
    });
  });
});

test.describe("Teams setup with email verification", () => {
  test.use({
    mockOptions: {
      overrides: {
        teams_status: () => ({ identity: null, unlocked: false, fingerprint: null, teams: [] }),
        // First attempt: the server wants an emailed code. The retry succeeds
        // only with the expected one; anything else is rejected (no new mail).
        teams_init: (args) => {
          // No code yet: the server asks for one (and mails it).
          const code = args && typeof args === "object" && "emailCode" in args ? args.emailCode : null;
          if (code == null) {
            throw { kind: "email_code_sent", email: "ana@example.com", expires_in_secs: 600 };
          }
          if (code !== "123456") {
            throw { kind: "email_code_rejected", email: "ana@example.com" };
          }
          return {
            identity: { email: "ana@example.com", server_url: "https://teams.example.com", device_id: "d-2" },
            unlocked: true,
            // Overrides are stringified into the page: no test-scope values.
            fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB",
            teams: [],
          };
        },
        teams_request_email_code: () => ({ expires_in_secs: 600 }),
      },
    },
  });

  test("asks for the emailed code, says when it is wrong, and retries", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByLabel("Sync server").fill("https://teams.example.com");
    await page.getByLabel("Your email").fill("ana@example.com");
    await page.getByLabel("Passphrase", { exact: true }).fill("correct horse");
    await page.getByLabel("Repeat passphrase").fill("correct horse");
    await page.getByRole("button", { name: "Set up this device" }).click();

    const notice = "The server sent a code to ana@example.com (valid 10 minutes).";
    await expect(page.getByText(notice)).toBeVisible();
    const code = page.getByLabel("Verification code");
    await expect(code).toBeVisible();

    await code.fill("000000");
    await page.getByRole("button", { name: "Set up this device" }).click();
    await expect(page.getByText("That code is wrong, expired or already used.")).toBeVisible();
    await expect(code).toHaveValue("");

    await page.getByRole("button", { name: "Send a new code" }).click();
    await expect(page.getByText(notice)).toBeVisible();
    await code.fill("123456");
    await page.getByRole("button", { name: "Set up this device" }).click();

    await expect(page.locator("code.fingerprint").first()).toHaveText(ANA_FP);
    const calls = (await app.ipcCalls("teams_init")) as Array<Record<string, unknown>>;
    expect(calls[0]).toMatchObject({ emailCode: null });
    expect(calls.at(-1)).toMatchObject({ emailCode: "123456" });
    expect(await app.ipcCalls("teams_request_email_code")).toHaveLength(1);
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
        teams_unlock: (args) => {
          if ((args as { passphrase: string }).passphrase !== "correct horse") throw "wrong Teams passphrase";
          return {
            identity: { email: "admin@example.com", server_url: "https://teams.example.com", device_id: "d-1" },
            unlocked: true,
            fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
            teams: [],
          };
        },
        teams_attention: () => [],
      },
    },
  });

  test("says inline when the passphrase is wrong, then unlocks", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await expect(page.getByRole("heading", { name: "Unlock Teams" })).toBeVisible();
    await expect(page.getByText("Team hosts still connect while locked")).toBeVisible();

    await page.getByLabel("Teams passphrase").fill("nope");
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByRole("alert")).toHaveText("That passphrase didn't unlock this device's keys.");

    await page.getByLabel("Teams passphrase").fill("correct horse");
    await page.getByRole("button", { name: "Unlock" }).click();
    await expect(page.getByText("No teams yet")).toBeVisible();
    await expect(page.getByRole("button", { name: "Lock" })).toBeVisible();
  });
});

test.describe("Teams join", () => {
  test.use({
    mockOptions: {
      overrides: {
        teams_status: () => ({
          identity: { email: "ana@example.com", server_url: "https://teams.example.com", device_id: "d-2" },
          unlocked: true,
          fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB",
          teams: [],
        }),
        teams_attention: () => [],
        teams_join: (args) => {
          const a = args as { token: string | null; teamId: string | null; fingerprint: string };
          if (a.fingerprint !== "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA") {
            throw (
              "no admin device of team 0b6f7c1e-1111-4222-8333-944455556666 has fingerprint " +
              a.fingerprint +
              ". Nothing was trusted."
            );
          }
          return {
            team_id: "0b6f7c1e-1111-4222-8333-944455556666",
            name: "platform",
            my_fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB",
          };
        },
      },
    },
  });

  test("parses a pasted invite and retries a fingerprint mismatch by team id", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "Join with invite" }).first().click();
    const dialog = page.getByRole("dialog", { name: "Join a team" });

    await dialog.getByLabel("Invite message").fill(
      [
        'Join the "platform" team on Bayesian SSH.',
        "Server: https://teams.example.com",
        "Email: ana@example.com",
        "Token: one-time-token-123",
        "Admin fingerprint: SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAX",
      ].join("\n"),
    );
    await expect(dialog.getByLabel("Invite token")).toHaveValue("one-time-token-123");
    const fp = dialog.getByLabel("Admin's fingerprint (from the invite)");
    await expect(fp).toHaveValue("SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAX");

    await dialog.getByRole("button", { name: "Join" }).click();
    await expect(dialog.getByRole("alert")).toContainText("doesn't belong to an admin of this team");
    await expect(dialog.getByRole("alert")).toContainText("0b6f7c1e-1111-4222-8333-944455556666");

    // Missing prefix and stray spaces are normalized.
    await fp.fill(" adminFingerprint AAAAAAAAAAAAAAAAAAAAAAAAAA ");
    await dialog.getByRole("button", { name: "Try again" }).click();
    await expect(page.getByRole("dialog", { name: "Joined platform" })).toContainText(
      "Waiting for an admin to grant this device the key.",
    );

    const calls = (await app.ipcCalls("teams_join")) as Array<Record<string, unknown>>;
    expect(calls).toHaveLength(2);
    expect(calls[0]).toMatchObject({ token: "one-time-token-123", teamId: null });
    expect(calls[1]).toMatchObject({
      token: null,
      teamId: "0b6f7c1e-1111-4222-8333-944455556666",
      fingerprint: ADMIN_FP,
    });
  });
});

const adminOverrides = {
  teams_status: () => ({
    identity: { email: "admin@example.com", server_url: "https://teams.example.com", device_id: "d-1" },
    unlocked: true,
    fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
    teams: [{ team_id: "t-1", source_id: "s-1", name: "platform", hosts: 2, last_sync_at: null }],
  }),
  teams_attention: () => [
    { team_id: "t-1", role: "admin", pending_devices: 1, waiting_for_key: false, removed: false },
  ],
  teams_members: () => [
    {
      email: "admin@example.com",
      role: "admin",
      devices: [{ name: "laptop", fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA" }],
    },
    {
      email: "ana@example.com",
      role: "member",
      devices: [{ name: "ana-laptop", fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB" }],
    },
  ],
  teams_pending: () => [
    {
      email: "ana@example.com",
      device: { name: "ana-laptop", fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB" },
    },
  ],
  teams_grant: (args: unknown) => ({
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
};

test.describe("Teams admin", () => {
  test.use({ mockOptions: { overrides: adminOverrides } });

  test("lists the team with its role and what needs attention", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    const row = page.getByRole("row", { name: /platform/ });
    await expect(row).toContainText("Admin");
    await expect(row).toContainText("1 device waiting");
    await expect(row).toContainText("never");
  });

  test("grant modal never shows the server's fingerprint before a match", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    await expect(page.getByRole("button", { name: "Verify and grant…" })).toBeVisible();
    // The waiting device's fingerprint is hidden in the member list too.
    await expect(page.getByText("Waiting for the key").first()).toBeVisible();
    expect(await page.content()).not.toContain("anaDeviceFingerprint");

    await page.getByRole("button", { name: "Verify and grant…" }).click();
    const dialog = page.getByRole("dialog", { name: "Grant the platform key to a new device" });
    const grant = dialog.getByRole("button", { name: "Grant key to ana-laptop" });
    const input = dialog.getByLabel("Fingerprint ana@example.com sent you");
    await expect(grant).toBeDisabled();
    await expect(dialog.getByText("Paste the fingerprint ana@example.com sent you")).toBeVisible();
    expect(await page.content()).not.toContain("anaDeviceFingerprint");

    // Mismatch: the button stays disabled and the reason says why.
    await input.fill("SHA256:somethingElseEntirelyCCCCCCCCCCCCCCCCCCCCCC");
    await expect(dialog.getByText(/Doesn't match\. Don't grant\./)).toBeVisible();
    await expect(dialog.getByText("The fingerprint doesn't match")).toBeVisible();
    await expect(grant).toBeDisabled();
    expect(await page.content()).not.toContain("anaDeviceFingerprint");

    // Match (chunked read-back, no prefix): still gated by the checkbox.
    await input.fill("anaD evic eFin gerp rint BBBB BBBB BBBB BBBB BBBB BBB");
    await expect(dialog.getByText("Matches the device waiting on the server.")).toBeVisible();
    await expect(dialog.locator("code.fingerprint")).toHaveText(ANA_FP);
    await expect(grant).toBeDisabled();
    await expect(dialog.getByText("Confirm where the fingerprint came from")).toBeVisible();
    await dialog.getByLabel("I got this fingerprint from ana@example.com directly, not from this screen.").check();
    await expect(grant).toBeEnabled();
    await expect(dialog).toContainText(`bssh teams grant platform --fingerprint ${ANA_FP}`);
    await grant.click();

    await expect(page.getByText("Granted the platform key to ana@example.com")).toBeVisible();
    const calls = (await app.ipcCalls("teams_grant")) as Array<Record<string, unknown>>;
    expect(calls).toEqual([{ team: "t-1", fingerprint: ANA_FP }]);
  });

  test("invite shows a ready-to-send message", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    await page.getByRole("button", { name: "Invite", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Invite someone to platform" });
    await dialog.getByLabel("Their email").fill("ana@example.com");
    await dialog.getByRole("button", { name: "Create invite" }).click();
    await expect(dialog.locator("pre")).toContainText(`bssh teams join one-time-token-123 --fingerprint ${ADMIN_FP}`);
    await expect(dialog.locator("pre")).toContainText("Join with invite");
    await expect(dialog.locator("pre")).toContainText("until 2030-01-08 00:00 UTC");
    await expect(dialog.getByRole("button", { name: "Copy message" })).toBeVisible();
  });
});

test.describe("Teams pending invites", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_pending: () => [],
        // Stateful across calls: a revoke empties the list.
        teams_invites: () =>
          (window as unknown as { __revoked?: boolean }).__revoked
            ? []
            : [{ email: "lost@example.com", role: "member", expires_at: "2026-10-29T22:00:00Z" }],
        teams_uninvite: () => {
          (window as unknown as { __revoked?: boolean }).__revoked = true;
          return null;
        },
      },
    },
  });

  test("lists a pending invite and revokes it", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    const section = page.getByRole("region", { name: /Pending invites/ });
    await expect(section).toContainText("lost@example.com");
    await expect(section).toContainText("holds a seat");
    await section.getByRole("button", { name: "Revoke the invite for lost@example.com" }).click();
    await expect(section).toHaveCount(0);
    const calls = await app.ipcCalls("teams_uninvite");
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({ team: "t-1", email: "lost@example.com" });
  });
});

test.describe("Teams remove member, blocked", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_pending: () => [],
        teams_remove_preview: () => ({
          email: "ana@example.com",
          removed_devices: [{ email: "ana@example.com", name: "ana-laptop", fingerprint: "SHA256:x", approved: true }],
          next_key_version: 4,
          recipients: [{ email: "admin@example.com", name: "laptop", fingerprint: "SHA256:y", approved: true }],
          blocking: [{ email: "bob@example.com", name: "unknown-01", fingerprint: "SHA256:z", approved: false }],
          shared_hosts: 3,
        }),
      },
    },
  });

  test("lists the blocking device and doesn't allow removal", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    await page.getByRole("button", { name: "Remove ana@example.com" }).click();
    const dialog = page.getByRole("dialog", { name: "Remove ana@example.com from platform?" });
    await expect(dialog).toContainText("rotates the team key to v4");
    await expect(dialog).toContainText("They keep copies of 3 hosts already synced: rotate credentials that matter.");
    await expect(dialog.getByRole("alert")).toContainText("bob@example.com · unknown-01");
    await expect(dialog.getByLabel("Type ana@example.com to confirm")).toHaveCount(0);
    await expect(dialog.getByRole("button", { name: "Remove and rotate key" })).toBeDisabled();
    await expect(dialog.getByText("Unapproved devices block the key rotation")).toBeVisible();
    expect(await app.ipcCalls("teams_remove")).toHaveLength(0);
  });
});

test.describe("Teams remove member, rotation fails", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_pending: () => [],
        teams_remove_preview: () => ({
          email: "ana@example.com",
          removed_devices: [{ email: "ana@example.com", name: "ana-laptop", fingerprint: "SHA256:x", approved: true }],
          next_key_version: 2,
          recipients: [],
          blocking: [],
          shared_hosts: 1,
        }),
        teams_remove: () => {
          throw "can't rotate the team key: these devices were never approved on this machine: unknown-01";
        },
      },
    },
  });

  test("needs the typed email and keeps a rotation failure in the modal", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    await page.getByRole("button", { name: "Remove ana@example.com" }).click();
    const dialog = page.getByRole("dialog", { name: "Remove ana@example.com from platform?" });
    const remove = dialog.getByRole("button", { name: "Remove and rotate key" });
    await expect(remove).toBeDisabled();
    await dialog.getByLabel("Type ana@example.com to confirm").fill("ana@example.co");
    await expect(remove).toBeDisabled();
    await dialog.getByLabel("Type ana@example.com to confirm").fill("ana@example.com");
    await remove.click();
    await expect(dialog.getByRole("alert")).toContainText("Rotation stopped. ana@example.com isn't fully removed yet.");
    await expect(dialog).toBeVisible();
  });
});

test.describe("Teams member role", () => {
  test.use({
    mockOptions: {
      overrides: {
        teams_status: () => ({
          identity: { email: "ana@example.com", server_url: "https://teams.example.com", device_id: "d-2" },
          unlocked: true,
          fingerprint: "SHA256:anaDeviceFingerprintBBBBBBBBBBBBBBBBBBBBBBB",
          teams: [{ team_id: "t-1", source_id: "s-1", name: "platform", hosts: 0, last_sync_at: null }],
        }),
        teams_attention: () => [
          { team_id: "t-1", role: "member", pending_devices: 0, waiting_for_key: true, removed: false },
        ],
        teams_members: () => [
          { email: "admin@example.com", role: "admin", devices: [] },
          { email: "ana@example.com", role: "member", devices: [] },
        ],
      },
    },
  });

  test("hides admin actions and shows the waiting-for-key status", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await expect(page.getByText("platform: waiting for an admin to grant this device the key.")).toBeVisible();
    await expect(page.getByRole("row", { name: /platform/ })).toContainText("Member");

    await page.getByRole("button", { name: "platform", exact: true }).click();
    await expect(page.getByText("Waiting for an admin to grant this device the key.")).toBeVisible();
    await expect(page.getByRole("tab")).toHaveText(["Hosts", "Members & devices"]);
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    await expect(page.getByText("admin@example.com")).toBeVisible();
    await expect(page.getByRole("button", { name: "Invite", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: /^Remove/ })).toHaveCount(0);
    await expect(page.getByText("Devices waiting for the key")).toHaveCount(0);
    expect(await app.ipcCalls("teams_pending")).toHaveLength(0);
  });
});

test.describe("Teams audit log, verified", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_audit: () => ({
          lines: [
            {
              occurred_at: "2030-01-02T09:30:00Z",
              received_at: "2030-01-02T09:31:00Z",
              kind: "session_start",
              who: "ana@example.com",
              device_name: "ana-laptop",
              host: "db-prod",
              client_ip: "203.0.113.7",
              check: "valid",
            },
          ],
          chains: 1,
          verified_events: 1,
          problems: [],
          invalid_events: 0,
          complete: true,
          clean: true,
        }),
      },
    },
  });

  test("verifies on open, shows UTC times and filters by member", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: "Audit log" }).click();
    await expect(page.getByRole("status").filter({ hasText: "Verified on this device" })).toContainText(
      "1 event, 1 device chain, the whole log, complete from each device's first event.",
    );
    const row = page.getByRole("row", { name: /ana@example.com/ });
    await expect(row).toContainText("Valid");
    await expect(row).toContainText("2030-01-02 09:30");
    await expect(row).toContainText("db-prod");
    await expect(row).toContainText("Session start");

    await page.getByLabel("Member").selectOption("ana@example.com");
    await page.getByLabel("Since (UTC)").fill("2030-01-01");
    await expect(page.getByText("bssh teams audit platform --member ana@example.com --since 2030-01-01")).toBeVisible();
    await expect
      .poll(async () => ((await app.ipcCalls("teams_audit")) as Array<Record<string, unknown>>).at(-1))
      .toMatchObject({ team: "t-1", member: "ana@example.com", since: "2030-01-01T00:00:00Z" });
  });
});

test.describe("Teams audit log, limited list", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_audit: () => ({
          lines: [],
          chains: 1,
          verified_events: 1,
          problems: [],
          invalid_events: 0,
          complete: false,
          clean: true,
        }),
      },
    },
  });

  test("says the list is cut but the whole log was verified", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: "Audit log" }).click();
    await expect(page.getByRole("status").filter({ hasText: "Verified on this device" })).toContainText(
      "the rest were verified too",
    );
  });
});

test.describe("Teams audit log, tampered", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_audit: () => ({
          lines: [
            {
              occurred_at: "2030-01-02T09:30:00Z",
              received_at: "2030-01-02T09:31:00Z",
              kind: "session_start",
              who: "ana@example.com",
              device_name: "ana-laptop",
              host: "db-prod",
              client_ip: "203.0.113.7",
              check: "bad_signature",
            },
          ],
          chains: 0,
          verified_events: 1,
          problems: ["device d-2: events 3 to 4 are missing"],
          invalid_events: 1,
          complete: true,
          clean: false,
        }),
      },
    },
  });

  test("flags failed verification as tampering and keeps it on the team row", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: "Audit log" }).click();
    const alert = page.getByRole("alert").filter({ hasText: "Verification failed" });
    await expect(alert).toContainText("treat this log as tampered");
    await expect(alert).toContainText("1 event failed the check · 1 chain problem");
    await expect(alert).toContainText("events 3 to 4 are missing");
    await expect(page.getByRole("row", { name: /ana@example.com/ })).toContainText("Bad signature");

    await page.getByRole("button", { name: "All teams" }).click();
    await expect(page.getByRole("row", { name: /platform/ })).toContainText("Audit log failed");
  });
});

test.describe("Teams billing, admin on a full free plan", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_billing: () => ({
          billing_enabled: true, plan: "free", free_seats: 3, paid_seats: 0, seat_limit: 3, seats_used: 3,
          status: null, interval: null, current_period_end: null, cancel_at_period_end: false, read_only: false,
        }),
        teams_billing_checkout: () => "https://checkout.stripe.test/c/pay_1",
      },
    },
  });

  test("upgrade opens Stripe Checkout for at least one seat above the free plan", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    const card = page.getByRole("region", { name: "Plan and seats" });
    await expect(card.getByText("Free plan · 3 seats")).toBeVisible();
    await expect(card.getByText(/3 of 3 seats used/)).toBeVisible();
    await expect(card.getByText(/All free seats are taken: upgrade/)).toBeVisible();
    await expect(card.getByRole("meter", { name: "Seats used" })).toHaveAttribute("aria-valuenow", "3");

    await card.getByRole("button", { name: "Upgrade…" }).click();
    await expect(card.getByLabel("Seats", { exact: true })).toHaveValue("4");
    await card.getByLabel("Yearly").check();
    await expect(card.getByText("bssh teams billing platform --checkout --seats 4 --yearly")).toBeVisible();
    await card.getByRole("button", { name: "Continue to payment" }).click();
    await app.waitForIpc("teams_billing_checkout", 1);
    expect((await app.ipcCalls("teams_billing_checkout"))[0]).toEqual({ team: "t-1", seats: 4, yearly: true });
    await expect(page.getByText(/Checkout opened in your browser/)).toBeVisible();
  });
});

test.describe("Teams billing, member of a read-only team", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_attention: () => [
          { team_id: "t-1", role: "member", pending_devices: 0, waiting_for_key: false, removed: false },
        ],
        teams_members: () => [
          { email: "admin@example.com", role: "member", devices: [] },
        ],
        teams_billing: () => ({
          billing_enabled: true, plan: "free", free_seats: 3, paid_seats: 0, seat_limit: 3, seats_used: 5,
          status: "canceled", interval: "month", current_period_end: null, cancel_at_period_end: false, read_only: true,
        }),
      },
    },
  });

  test("says the team is read-only and offers no billing actions", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    const card = page.getByRole("region", { name: "Plan and seats" });
    await expect(card.getByRole("alert")).toContainText("This team is read-only");
    await expect(card.getByRole("alert")).toContainText("no active subscription");
    await expect(card.getByRole("alert")).toContainText("an admin subscribes or removes members");
    await expect(card.getByRole("button", { name: "Upgrade…" })).toHaveCount(0);
    await expect(card.getByRole("button", { name: "Manage billing" })).toHaveCount(0);
  });
});


test.describe("Teams billing, admin of a team with too few paid seats", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminOverrides,
        teams_billing: () => ({
          billing_enabled: true, plan: "team", free_seats: 3, paid_seats: 4, seat_limit: 4, seats_used: 6,
          status: "past_due", interval: "month", current_period_end: "2026-12-01T00:00:00Z", cancel_at_period_end: true, read_only: true,
        }),
      },
    },
  });

  test("names the cause, the failed payment and the end of the plan", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    const card = page.getByRole("region", { name: "Plan and seats" });
    await expect(card.getByRole("alert")).toContainText("more members than paid seats (4)");
    await expect(card.getByRole("alert")).toContainText("add seats with Manage billing or remove members");
    await expect(card).toContainText("Payment failed; Stripe is retrying. Update the card with Manage billing.");
    await expect(card).toContainText("then the team falls back to the free plan");
    await expect(card.getByRole("button", { name: "Manage billing" })).toBeVisible();
  });
});
