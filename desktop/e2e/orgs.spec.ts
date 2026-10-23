import { test, expect } from "./fixtures/app";

/**
 * Organizations on the Teams page: the shared-seats line of the team's plan
 * card, the Organization page (owners, teams, plan), the team creation picker
 * and a server without organizations. Overrides are stringified into the
 * page, so they are self-contained (no closing over this scope).
 */

const ORG_A = "11111111-1111-4111-8111-111111111111";
const ORG_B = "22222222-2222-4222-8222-222222222222";

/** Admin of team "platform"; every describe adds its organization commands. */
const adminBase = {
  teams_status: () => ({
    identity: { email: "ana@example.com", server_url: "https://teams.example.com", device_id: "d-1" },
    unlocked: true,
    fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
    teams: [{ team_id: "t-1", source_id: "s-1", name: "platform", hosts: 2, last_sync_at: null }],
  }),
  teams_attention: () => [{ team_id: "t-1", role: "admin", pending_devices: 0, waiting_for_key: false, removed: false }],
  teams_members: () => [
    { email: "ana@example.com", role: "admin", devices: [{ name: "laptop", fingerprint: "SHA256:aaaa" }] },
  ],
};

test.describe("Organizations, owner", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminBase,
        teams_orgs: () => [
          {
            org_id: "11111111-1111-4111-8111-111111111111",
            short_id: "11111111",
            label: "ana@example.com, bo@example.com",
            my_role: "owner",
            owners: [
              { account_id: "a-1", email: "ana@example.com" },
              { account_id: "a-2", email: "bo@example.com" },
            ],
            teams: [{ team_id: "t-1", name: "platform", short_id: "t1t1t1t1" }],
            created_at: "2030-01-01T00:00:00Z",
          },
        ],
        teams_org: () => ({
          org_id: "11111111-1111-4111-8111-111111111111",
          short_id: "11111111",
          label: "ana@example.com, bo@example.com",
          my_role: "owner",
          owners: [
            { account_id: "a-1", email: "ana@example.com" },
            { account_id: "a-2", email: "bo@example.com" },
          ],
          teams: [{ team_id: "t-1", name: "platform", short_id: "t1t1t1t1" }],
          created_at: "2030-01-01T00:00:00Z",
        }),
        teams_billing: () => ({
          billing_enabled: true, plan: "free", free_seats: 3, paid_seats: 0, seat_limit: 3, seats_used: 3,
          status: null, interval: null, current_period_end: null, cancel_at_period_end: false, read_only: false,
          org_id: "11111111-1111-4111-8111-111111111111",
        }),
        teams_org_billing: () => ({
          billing_enabled: true, plan: "free", free_seats: 3, paid_seats: 0, seat_limit: 3, seats_used: 3,
          status: null, interval: null, current_period_end: null, cancel_at_period_end: false, read_only: false,
          org_id: "11111111-1111-4111-8111-111111111111",
        }),
        teams_org_billing_checkout: () => "https://checkout.stripe.test/c/org_1",
        teams_org_add_owner: (args: unknown) => ({
          org_id: "11111111-1111-4111-8111-111111111111",
          short_id: "11111111",
          label: "ana@example.com, bo@example.com, cy@example.com",
          my_role: "owner",
          owners: [
            { account_id: "a-1", email: "ana@example.com" },
            { account_id: "a-2", email: "bo@example.com" },
            { account_id: "a-3", email: (args as { email: string }).email },
          ],
          teams: [],
          created_at: "2030-01-01T00:00:00Z",
        }),
        teams_org_remove_owner: () => ({
          org_id: "11111111-1111-4111-8111-111111111111",
          short_id: "11111111",
          label: "ana@example.com",
          my_role: "owner",
          owners: [{ account_id: "a-1", email: "ana@example.com" }],
          teams: [],
          created_at: "2030-01-01T00:00:00Z",
        }),
      },
    },
  });

  test("the team's plan card says seats are shared and checkout targets the organization", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    const card = page.getByRole("region", { name: "Plan and seats" });
    await expect(card.getByTestId("org-seats-note")).toContainText(
      "Seats are shared by the team's organization (ana@example.com, bo@example.com)",
    );
    await expect(card.getByText(/3 of 3 seats used/)).toBeVisible();
    await card.getByRole("button", { name: "Upgrade…" }).click();
    await expect(card.getByText("bssh org billing 11111111 --checkout --seats 4")).toBeVisible();
    await card.getByRole("button", { name: "Continue to payment" }).click();
    await app.waitForIpc("teams_org_billing_checkout", 1);
    expect((await app.ipcCalls("teams_org_billing_checkout"))[0]).toEqual({ org: ORG_A, seats: 4, yearly: false });
    expect(await app.ipcCalls("teams_billing_checkout")).toHaveLength(0);
  });

  test("the organization page lists owners and edits them", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "ana@example.com, bo@example.com", exact: true }).click();
    await expect(page.getByRole("heading", { name: "ana@example.com, bo@example.com", level: 2 })).toBeVisible();
    await expect(page.getByRole("region", { name: "Owners" })).toContainText("bo@example.com");

    await page.getByLabel("Add an owner").fill("cy@example.com");
    await page.getByRole("button", { name: "Add owner" }).click();
    await app.waitForIpc("teams_org_add_owner", 1);
    expect((await app.ipcCalls("teams_org_add_owner"))[0]).toEqual({ org: ORG_A, email: "cy@example.com" });

    await page.getByRole("button", { name: "Remove owner bo@example.com" }).click();
    await page.getByRole("button", { name: "Confirm remove" }).click();
    await app.waitForIpc("teams_org_remove_owner", 1);
    expect((await app.ipcCalls("teams_org_remove_owner"))[0]).toEqual({ org: ORG_A, email: "bo@example.com" });
  });
});

test.describe("Organizations, last owner", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminBase,
        teams_orgs: () => [
          {
            org_id: "11111111-1111-4111-8111-111111111111",
            short_id: "11111111",
            label: "ana@example.com",
            my_role: "owner",
            owners: [{ account_id: "a-1", email: "ana@example.com" }],
            teams: [
              { team_id: "t-1", name: "platform", short_id: "t1t1t1t1" },
              { team_id: "t-9", name: null, short_id: "99999999" },
            ],
            created_at: "2030-01-01T00:00:00Z",
          },
        ],
        teams_org: () => ({
          org_id: "11111111-1111-4111-8111-111111111111",
          short_id: "11111111",
          label: "ana@example.com",
          my_role: "owner",
          owners: [{ account_id: "a-1", email: "ana@example.com" }],
          teams: [],
          created_at: "2030-01-01T00:00:00Z",
        }),
        teams_org_billing: () => ({
          billing_enabled: true, plan: "team", free_seats: 3, paid_seats: 5, seat_limit: 5, seats_used: 2,
          status: "active", interval: "month", current_period_end: "2030-02-01T00:00:00Z",
          cancel_at_period_end: false, read_only: false, org_id: "11111111-1111-4111-8111-111111111111",
        }),
        teams_org_billing_portal: () => "https://billing.stripe.test/p/org_1",
      },
    },
  });

  test("can't be removed; teams show local names or short ids; the portal is the organization's", async ({
    app,
    page,
  }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "ana@example.com", exact: true }).click();
    await expect(page.getByRole("button", { name: /Remove owner/ })).toHaveCount(0);
    const teams = page.getByRole("region", { name: "Teams", exact: true });
    await expect(teams.getByRole("button", { name: "platform" })).toBeVisible();
    await expect(teams).toContainText("99999999");
    const card = page.getByRole("region", { name: "Plan and seats" });
    await expect(card.getByText("Team plan · 5 seats, billed monthly")).toBeVisible();
    await card.getByRole("button", { name: "Manage billing" }).click();
    await app.waitForIpc("teams_org_billing_portal", 1);
    expect((await app.ipcCalls("teams_org_billing_portal"))[0]).toEqual({ org: ORG_A });
  });
});

test.describe("Organizations, member and not an owner", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminBase,
        teams_attention: () => [
          { team_id: "t-1", role: "member", pending_devices: 0, waiting_for_key: false, removed: false },
        ],
        teams_orgs: () => [
          {
            org_id: "11111111-1111-4111-8111-111111111111",
            short_id: "11111111",
            label: "bo@example.com",
            my_role: "member",
            owners: [{ account_id: "a-2", email: "bo@example.com" }],
            teams: [{ team_id: "t-1", name: "platform", short_id: "t1t1t1t1" }],
            created_at: "2030-01-01T00:00:00Z",
          },
        ],
        teams_org: () => ({
          org_id: "11111111-1111-4111-8111-111111111111",
          short_id: "11111111",
          label: "bo@example.com",
          my_role: "member",
          owners: [{ account_id: "a-2", email: "bo@example.com" }],
          teams: [{ team_id: "t-1", name: "platform", short_id: "t1t1t1t1" }],
          created_at: "2030-01-01T00:00:00Z",
        }),
        teams_org_billing: () => ({
          billing_enabled: true, plan: "free", free_seats: 3, paid_seats: 0, seat_limit: 3, seats_used: 5,
          status: null, interval: null, current_period_end: null, cancel_at_period_end: false, read_only: true,
          org_id: "11111111-1111-4111-8111-111111111111",
        }),
        teams_billing: () => ({
          billing_enabled: true, plan: "free", free_seats: 3, paid_seats: 0, seat_limit: 3, seats_used: 5,
          status: null, interval: null, current_period_end: null, cancel_at_period_end: false, read_only: true,
          org_id: "11111111-1111-4111-8111-111111111111",
        }),
      },
    },
  });

  test("shows who the owners are and offers no billing or owner actions", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    const card = page.getByRole("region", { name: "Plan and seats" });
    await expect(card.getByTestId("org-seats-note")).toContainText("Billing is managed by its owners: bo@example.com");
    await expect(card.getByRole("button", { name: "Manage billing" })).toHaveCount(0);
    await expect(card.getByRole("button", { name: /Upgrade/ })).toHaveCount(0);
    await expect(card.getByRole("alert")).toContainText("an organization owner subscribes or removes members");

    await page.getByRole("button", { name: "All teams" }).click();
    await page.getByRole("button", { name: "bo@example.com", exact: true }).click();
    await expect(page.getByLabel("Add an owner")).toHaveCount(0);
    await expect(page.getByText("Only owners can change owners and billing.")).toBeVisible();
  });
});

test.describe("Organizations, team creation with several owned", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminBase,
        teams_orgs: () => [
          {
            org_id: "22222222-2222-4222-8222-222222222222",
            short_id: "22222222",
            label: "ana@example.com",
            my_role: "owner",
            owners: [{ account_id: "a-1", email: "ana@example.com" }],
            teams: [],
            created_at: "2031-01-01T00:00:00Z",
          },
          {
            org_id: "11111111-1111-4111-8111-111111111111",
            short_id: "11111111",
            label: "ana@example.com, bo@example.com",
            my_role: "owner",
            owners: [{ account_id: "a-1", email: "ana@example.com" }],
            teams: [{ team_id: "t-1", name: "platform", short_id: "t1t1t1t1" }],
            created_at: "2030-01-01T00:00:00Z",
          },
          {
            org_id: "33333333-3333-4333-8333-333333333333",
            short_id: "33333333",
            label: "cy@example.com",
            my_role: "member",
            owners: [{ account_id: "a-3", email: "cy@example.com" }],
            teams: [],
            created_at: "2029-01-01T00:00:00Z",
          },
        ],
        teams_create: () => ({
          team_id: "t-2", name: "infra", waiting_for_key: false, pulled: 0, pushed: 0, conflicts_resolved: 0,
          audit_uploaded: 0, dropped_edits: 0, rejected: [], unreadable: 0, rotated_to: null,
          rotation_required: false, errors: [], hosts_added: 0, hosts_updated: 0, hosts_stale: 0,
        }),
      },
    },
  });

  test("offers only owned organizations, the oldest preselected", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "Create team" }).first().click();
    const picker = page.getByLabel("Organization", { exact: true });
    await expect(picker.locator("option")).toHaveCount(2);
    await expect(picker).toHaveValue(ORG_A);
    await page.getByLabel("Team name").fill("infra");
    await picker.selectOption(ORG_B);
    await page.getByRole("dialog").getByRole("button", { name: "Create team" }).click();
    await app.waitForIpc("teams_create", 1);
    expect((await app.ipcCalls("teams_create"))[0]).toEqual({ name: "infra", org: ORG_B });
  });
});

test.describe("Organizations, a single owned organization", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminBase,
        teams_orgs: () => [
          {
            org_id: "11111111-1111-4111-8111-111111111111",
            short_id: "11111111",
            label: "ana@example.com",
            my_role: "owner",
            owners: [{ account_id: "a-1", email: "ana@example.com" }],
            teams: [],
            created_at: "2030-01-01T00:00:00Z",
          },
        ],
      },
    },
  });

  test("shows no picker: the server uses the only organization", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await page.getByRole("button", { name: "Create team" }).first().click();
    await expect(page.getByLabel("Team name")).toBeVisible();
    await expect(page.getByLabel("Organization", { exact: true })).toHaveCount(0);
  });
});

test.describe("Organizations, server without them", () => {
  test.use({
    mockOptions: {
      overrides: {
        ...adminBase,
        teams_orgs: () => null,
        teams_billing: () => ({
          billing_enabled: true, plan: "free", free_seats: 3, paid_seats: 0, seat_limit: 3, seats_used: 3,
          status: null, interval: null, current_period_end: null, cancel_at_period_end: false, read_only: false,
        }),
        teams_billing_checkout: () => "https://checkout.stripe.test/c/pay_1",
      },
    },
  });

  test("hides the organization UI and keeps the team plan card working", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Teams");
    await expect(page.getByRole("button", { name: "platform", exact: true })).toBeVisible();
    await expect(page.getByRole("heading", { name: "Organizations" })).toHaveCount(0);

    await page.getByRole("button", { name: "platform", exact: true }).click();
    await page.getByRole("tab", { name: /Members & devices/ }).click();
    const card = page.getByRole("region", { name: "Plan and seats" });
    await expect(card.getByText("Free plan · 3 seats")).toBeVisible();
    await expect(card.getByTestId("org-seats-note")).toHaveCount(0);
    await card.getByRole("button", { name: "Upgrade…" }).click();
    await card.getByRole("button", { name: "Continue to payment" }).click();
    await app.waitForIpc("teams_billing_checkout", 1);
    expect((await app.ipcCalls("teams_billing_checkout"))[0]).toEqual({ team: "t-1", seats: 4, yearly: false });
  });
});
