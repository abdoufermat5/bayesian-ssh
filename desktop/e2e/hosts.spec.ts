import { test, expect } from "./fixtures/app";
import type { Page } from "./fixtures/app";

/**
 * Hosts / Connections view end-to-end coverage.
 *
 * All expectations target user-visible behaviour: rendered rows, modals,
 * toasts and the recorded IPC payloads (the mock documents the exact shapes).
 */

const HOSTS = [
  "prod-api-01",
  "prod-api-02",
  "prod-db-primary",
  "staging-web",
  "build-runner",
  "homelab-nas",
  "gpu-trainer",
  "edge-proxy-eu",
];

/** Title attribute of the status dot is a user-visible reachability label. */
const row = (page: Page, name: string) =>
  page.locator("tbody tr").filter({ hasText: name });

test.describe("hosts list", () => {
  test("renders every mocked host", async ({ app, page }) => {
    await app.gotoApp();
    await expect(page.locator(".workspace h1")).toHaveText("Hosts");
    await expect(page.locator("tbody tr")).toHaveCount(HOSTS.length);
    for (const name of HOSTS) {
      await expect(row(page, name)).toHaveCount(1);
    }
  });

  test("typing filters: one debounced get_connections call, rows narrow", async ({ app, page }) => {
    await app.gotoApp();
    await app.clearIpc();

    const input = page.locator(".search-input");
    await input.click();
    // Several keystrokes well inside the 150ms debounce window.
    await input.pressSequentially("prod", { delay: 10 });

    // The final IPC call carries the whole query, not one call per keystroke.
    await expect
      .poll(async () => {
        const calls = (await app.ipcCalls("get_connections")) as Array<{ query: string }>;
        return calls.length ? calls[calls.length - 1].query : null;
      })
      .toBe("prod");
    const calls = (await app.ipcCalls("get_connections")) as Array<{ query: string }>;
    expect(calls.length).toBeLessThanOrEqual(1);

    await expect(page.locator("tbody tr")).toHaveCount(3);
    await expect(row(page, "prod-api-01")).toHaveCount(1);
    await expect(row(page, "prod-db-primary")).toHaveCount(1);
    await expect(row(page, "staging-web")).toHaveCount(0);
    await expect(page.locator(".view-header")).toContainText("3 matching");
  });

  test("a tag chip filters via get_connections tagFilter", async ({ app, page }) => {
    await app.gotoApp();
    await app.clearIpc();

    const tags = page.getByRole("group", { name: "Filter by tag" });
    await tags.getByRole("button", { name: "prod", exact: true }).click();

    await expect
      .poll(async () => {
        const c = (await app.ipcCalls("get_connections")) as Array<{ tagFilter: string | null }>;
        return c.length ? c[c.length - 1].tagFilter : null;
      })
      .toBe("prod");
    await expect(page.locator("tbody tr")).toHaveCount(4);

    // Clicking the active chip clears the tag filter.
    await tags.getByRole("button", { name: "prod", exact: true }).click();
    await expect
      .poll(async () => {
        const c = (await app.ipcCalls("get_connections")) as Array<{ tagFilter: string | null }>;
        return c.length ? c[c.length - 1].tagFilter : null;
      })
      .toBe(null);
    await expect(page.locator("tbody tr")).toHaveCount(HOSTS.length);
  });

  test("no-match query shows the empty state and Clear restores the list", async ({ app, page }) => {
    await app.gotoApp();
    await app.clearIpc();

    await page.locator(".search-input").fill("no-such-host-zzz");
    await expect
      .poll(async () => {
        const c = (await app.ipcCalls("get_connections")) as Array<{ query: string }>;
        return c.length ? c[c.length - 1].query : null;
      })
      .toBe("no-such-host-zzz");

    await expect(page.getByText("No hosts match")).toBeVisible();
    await expect(page.locator("tbody tr")).toHaveCount(0);

    await page.locator(".empty-state").getByRole("button", { name: "Clear filter" }).click();
    await expect(page.locator(".search-input")).toHaveValue("");
    await expect(page.locator("tbody tr")).toHaveCount(HOSTS.length);
    await expect(row(page, "prod-api-01")).toBeVisible();
  });

  test("sort control reorders the rows", async ({ app, page }) => {
    await app.gotoApp();
    await expect(page.locator("tbody tr").first()).toContainText("prod-api-01");

    await page.getByRole("combobox").click();
    await page.getByRole("option", { name: "Name", exact: true }).click();

    await expect(page.locator("tbody tr").first()).toContainText("build-runner");
    await expect(page.locator("tbody tr").last()).toContainText("staging-web");

    // Back to the default ranking restores the seeded order.
    await page.getByRole("combobox").click();
    await page.getByRole("option", { name: /^Smart rank/ }).click();
    await expect(page.locator("tbody tr").first()).toContainText("prod-api-01");
  });

  test("list/grid toggle switches the layout", async ({ app, page }) => {
    await app.gotoApp();
    await expect(page.locator("table")).toHaveCount(1);

    await page.getByRole("button", { name: "Grid layout" }).click();
    await expect(page.getByRole("button", { name: "Grid layout" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    // Hosts are rendered as cards, no table rows.
    await expect(page.locator("table")).toHaveCount(0);
    await expect(page.locator("tbody tr")).toHaveCount(0);
    await expect(page.locator("[data-host-selected]")).toHaveCount(HOSTS.length);
    await expect(page.getByText("edge-proxy-eu")).toBeVisible();

    await page.getByRole("button", { name: "List layout" }).click();
    await expect(page.locator("table")).toHaveCount(1);
    await expect(page.locator("tbody tr")).toHaveCount(HOSTS.length);
  });

  test("keyboard selection (ArrowDown/j) and Enter connect to the selected host", async ({
    app,
    page,
  }) => {
    await app.gotoApp();
    await page.locator(".view-title").click(); // ensure focus isn't in an input

    await page.keyboard.press("ArrowDown");
    await expect(page.locator('tr[data-host-selected="true"]')).toContainText("prod-api-02");

    await page.keyboard.press("j");
    await expect(page.locator('tr[data-host-selected="true"]')).toContainText("prod-db-primary");

    await page.keyboard.press("ArrowUp");
    await expect(page.locator('tr[data-host-selected="true"]')).toContainText("prod-api-02");

    await page.keyboard.press("Enter");
    await expect(page.locator(".workspace h1")).toHaveText("Terminals");
    await expect
      .poll(async () => {
        const c = (await app.ipcCalls("spawn_pty")) as Array<{
          sessionId: string;
          connectionName: string;
        }>;
        return c.length ? c[c.length - 1].connectionName : null;
      })
      .toBe("prod-api-02");
    const spawn = (await app.ipcCalls("spawn_pty")) as Array<{ sessionId: string }>;
    expect(spawn[spawn.length - 1].sessionId).toBeTruthy();
  });
});

/** Open a host's "More actions" menu and pick an item. */
async function rowMenu(page: Page, host: string, item: string) {
  await page.getByRole("button", { name: `More actions for ${host}` }).click();
  await page.getByRole("menu", { name: `Actions for ${host}` }).getByRole("menuitem", { name: item }).click();
}

/** Select a host row by clicking it (the row becomes the grid's focused tab stop). */
async function focusRow(page: Page, host: string) {
  await row(page, host).locator("td").first().click();
  await expect(row(page, host)).toBeFocused();
}

test.describe("hosts row actions", () => {
  test.use({ permissions: ["clipboard-read", "clipboard-write"] });

  test("actions are not repeated on every row: one menu per row, Connect and Files as icons", async ({ app, page }) => {
    await app.gotoApp();
    await expect(page.getByRole("button", { name: "Connect", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Connect to prod-api-01" })).toHaveCount(1);
    await expect(page.getByRole("button", { name: "Browse files on prod-api-01" })).toHaveCount(1);

    await page.getByRole("button", { name: "More actions for prod-api-01" }).click();
    const menu = page.getByRole("menu", { name: "Actions for prod-api-01" });
    await expect(menu.getByRole("menuitem")).toHaveText([
      /Copy ssh command/,
      /Edit/,
      /Duplicate/,
      /Share with a team…/,
      /Delete…/,
    ]);
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(page.getByRole("button", { name: "More actions for prod-api-01" })).toBeFocused();
  });

  test("copy SSH command puts it on the clipboard and toasts", async ({ app, page }) => {
    await app.gotoApp();
    await rowMenu(page, "prod-api-01", "Copy ssh command");

    await expect(
      page.getByRole("status").filter({ hasText: "SSH command copied to clipboard" }),
    ).toBeVisible();
    const clip = await page.evaluate(() => navigator.clipboard.readText());
    expect(clip).toContain("deploy@10.0.4.12");
    expect(clip).toContain("-J jump@bastion.corp");
  });

  test("duplicate creates a copy and opens the edit modal", async ({ app, page }) => {
    await app.gotoApp();
    await rowMenu(page, "prod-api-01", "Duplicate");

    await expect
      .poll(async () => {
        const c = (await app.ipcCalls("add_connection")) as Array<{ name: string; host: string }>;
        return c.length ? c[c.length - 1].name : null;
      })
      .toBe("prod-api-01 (Copy)");
    const adds = (await app.ipcCalls("add_connection")) as Array<{ name: string; host: string }>;
    expect(adds[adds.length - 1].host).toBe("10.0.4.12");

    const dialog = page.getByRole("dialog").filter({ hasText: "Edit host" });
    await expect(dialog).toBeVisible();
    await expect(dialog.locator("input").first()).toHaveValue("prod-api-01 (Copy)");
  });

  test("delete confirms then removes the connection", async ({ app, page }) => {
    await app.gotoApp();
    await app.clearIpc();
    await rowMenu(page, "prod-api-01", "Delete…");

    const dialog = page.getByRole("dialog").filter({ hasText: "Delete prod-api-01?" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Delete", exact: true }).click();

    await expect.poll(async () => app.ipcCalls("remove_connection")).toEqual([{ idOrName: "id-0" }]);
    await expect(page.locator("tbody tr")).toHaveCount(HOSTS.length - 1);
    await expect(row(page, "prod-api-01")).toHaveCount(0);
  });
});

test.describe("hosts keyboard", () => {
  test.use({ permissions: ["clipboard-read", "clipboard-write"] });

  test("the list is one tab stop; arrows reach a row's actions", async ({ app, page }) => {
    await app.gotoApp();
    const grid = page.getByRole("grid", { name: "Hosts" });
    await expect(grid.locator('tr[tabindex="0"]')).toHaveCount(1);

    await focusRow(page, "staging-web");
    await expect(grid.locator('tr[tabindex="0"]')).toContainText("staging-web");
    await expect(row(page, "staging-web")).toHaveAttribute("aria-selected", "true");

    await page.keyboard.press("ArrowRight");
    await expect(page.getByRole("button", { name: "Connect to staging-web" })).toBeFocused();
    await page.keyboard.press("ArrowRight");
    await expect(page.getByRole("button", { name: "Browse files on staging-web" })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(row(page, "staging-web")).toBeFocused();

    // K moves the selection up, and focus follows it.
    await page.keyboard.press("k");
    await expect(page.locator('tr[data-host-selected="true"]')).toContainText("prod-db-primary");
    await expect(row(page, "prod-db-primary")).toBeFocused();
  });

  test("C copies, E edits, Delete asks to delete the selected host", async ({ app, page }) => {
    await app.gotoApp();
    await focusRow(page, "prod-api-02");

    await page.keyboard.press("c");
    await expect(page.getByRole("status").filter({ hasText: "SSH command copied to clipboard" })).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toContain("deploy@10.0.4.13");

    await page.keyboard.press("e");
    const edit = page.getByRole("dialog").filter({ hasText: "Edit host" });
    await expect(edit).toBeVisible();
    await expect(page.locator("#c-name")).toHaveValue("prod-api-02");
    await page.keyboard.press("Escape");
    await expect(edit).toBeHidden();

    await focusRow(page, "prod-api-02");
    await page.keyboard.press("Delete");
    await expect(page.getByRole("dialog", { name: "Delete prod-api-02?" })).toBeVisible();
  });

  test("Shift+Enter connects in a tab; Ctrl+Enter opens Files", async ({ app, page }) => {
    await app.gotoApp();
    await focusRow(page, "staging-web");
    await page.keyboard.press("Control+Enter");
    await expect(page.locator(".workspace h1")).toHaveText("Files");

    await app.nav("Hosts");
    await focusRow(page, "staging-web");
    await page.keyboard.press("Shift+Enter");
    await expect(page.locator(".workspace h1")).toHaveText("Terminals");
    await expect
      .poll(async () => ((await app.ipcCalls("spawn_pty")) as Array<{ connectionName: string }>).at(-1)?.connectionName)
      .toBe("staging-web");
  });

  test("host keys don't fire while the filter has focus", async ({ app, page }) => {
    await app.gotoApp();
    await page.locator(".search-input").click();
    await page.keyboard.press("e");
    await page.keyboard.press("c");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.locator(".search-input")).toHaveValue("ec");
  });
});

test.describe("hosts ping and batch", () => {
  test.use({
    mockOptions: {
      overrides: {
        // The shipped mock has no ping handler; supply one with a fixed result.
        ping_all_connections: () =>
          [
            { connection_id: "id-0", success: true, latency_ms: 11 },
            { connection_id: "id-1", success: true, latency_ms: 14 },
            { connection_id: "id-2", success: true, latency_ms: 22 },
            { connection_id: "id-3", success: true, latency_ms: 31 },
            { connection_id: "id-4", success: true, latency_ms: 48 },
            { connection_id: "id-5", success: false, latency_ms: 0 },
            { connection_id: "id-6", success: false, latency_ms: 0 },
            { connection_id: "id-7", success: false, latency_ms: 0 },
          ],
      },
    },
  });

  test("Ping all reports reachable hosts", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "Ping", exact: true }).click();

    await expect.poll(async () => (await app.ipcCalls("ping_all_connections")).length).toBe(1);
    await expect(
      page.getByRole("status").filter({ hasText: "5 of 8 hosts reachable" }),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "5/8 up" })).toBeVisible();
    await expect(page.locator('[title^="Reachable ·"]')).toHaveCount(5);
    await expect(page.locator('[title="Unreachable"]')).toHaveCount(3);
  });

  test("Batch run opens the batch modal", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "Batch run" }).click();
    await expect(page.getByRole("dialog").filter({ hasText: "Run command on hosts" })).toBeVisible();
  });
});

test.describe("hosts sharing without Teams", () => {
  test("S on a host explains Teams isn't set up and offers to open Teams", async ({ app, page }) => {
    await app.gotoApp();
    await focusRow(page, "prod-api-01");
    await page.keyboard.press("s");

    const dialog = page.getByRole("dialog", { name: "Share prod-api-01 with a team" });
    await expect(dialog.getByText("Teams isn't set up on this device")).toBeVisible();
    await expect(dialog.getByRole("button", { name: /^Share/ })).toHaveCount(0);
    await dialog.getByRole("button", { name: "Open Teams" }).click();
    await expect(page.locator(".workspace h1")).toHaveText("Teams");
    expect(await app.ipcCalls("teams_share")).toEqual([]);
  });
});

/** Three hosts: one local, one from the team "platform", one synced from AWS. */
const provenanceOverrides = {
  get_connections: (args: unknown) => {
    const host = (name: string, address: string, extra: Record<string, unknown>) => ({
      id: "x-" + name,
      name,
      host: address,
      user: "ubuntu",
      port: 22,
      tags: [],
      use_kerberos: false,
      created_at: "2030-01-01T00:00:00Z",
      ...extra,
    });
    const all = [
      host("web-1", "10.0.0.1", { tags: ["prod", "web", "eu", "blue"], bastion: "jump.corp", key_path: "~/.ssh/id_ed25519" }),
      host("db-prod@platform", "10.0.6.10", { source_id: "s-1", external_id: "e-1" }),
      host("aws-runner", "10.1.0.5", { source_id: "s-aws" }),
    ];
    const q = (args as { query?: string } | null)?.query;
    return q ? all.filter((c) => c.name.includes(q)) : all;
  },
  list_sources: () => [
    { id: "s-1", name: "platform", kind: "team", config: {}, created_at: "2030-01-01T00:00:00Z" },
    { id: "s-aws", name: "aws-prod", kind: "aws", config: {}, created_at: "2030-01-01T00:00:00Z" },
  ],
  teams_status: () => ({
    identity: { email: "admin@example.com", server_url: "https://teams.example.com", device_id: "d-1" },
    unlocked: true,
    fingerprint: "SHA256:adminFingerprintAAAAAAAAAAAAAAAAAAAAAAAAAA",
    teams: [{ team_id: "t-1", source_id: "s-1", name: "platform", hosts: 1, last_sync_at: null }],
  }),
  teams_attention: () => [
    { team_id: "t-1", role: "admin", pending_devices: 0, waiting_for_key: false, removed: false },
  ],
  teams_share: () => ({
    team_id: "t-1",
    waiting_for_key: false,
    pulled: 0,
    pushed: 1,
    conflicts_resolved: 0,
    audit_uploaded: 0,
    dropped_edits: 0,
    rejected: [],
    unreadable: 0,
    rotation_required: false,
    errors: [],
    hosts_added: 1,
    hosts_updated: 0,
    hosts_stale: 0,
  }),
};

test.describe("hosts provenance and sharing", () => {
  test.use({ mockOptions: { overrides: provenanceOverrides } });

  test("source chips filter the list; team hosts carry a team badge", async ({ app, page }) => {
    await app.gotoApp();
    await expect(page.locator("tbody tr")).toHaveCount(3);
    const sources = page.getByRole("group", { name: "Filter by source" });

    await sources.getByRole("button", { name: /platform/ }).click();
    await expect(page.locator("tbody tr")).toHaveCount(1);
    await expect(row(page, "db-prod@platform").getByText("platform", { exact: true })).toBeVisible();
    await expect(row(page, "db-prod@platform").locator('[title^="Team platform"]')).toHaveCount(1);

    await sources.getByRole("button", { name: /^Local/ }).click();
    await expect(page.locator("tbody tr")).toHaveCount(1);
    await expect(row(page, "web-1")).toContainText("Local");

    await sources.getByRole("button", { name: /^All/ }).click();
    await expect(row(page, "aws-runner")).toContainText("aws-prod");
    await expect(row(page, "aws-runner").locator('[title^="Synced from aws-prod"]')).toHaveCount(1);
  });

  test("S shares a local host: what is shared, what never is, who sees it", async ({ app, page }) => {
    await app.gotoApp();
    await focusRow(page, "web-1");
    await page.keyboard.press("s");

    const dialog = page.getByRole("dialog", { name: "Share web-1 with a team" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("10.0.0.1")).toBeVisible();
    await expect(dialog.getByText("jump.corp")).toBeVisible();
    await expect(dialog.getByText("prod, web, eu, blue")).toBeVisible();
    await expect(dialog.getByText("key paths and passwords")).toBeVisible();
    await expect(dialog).not.toContainText("id_ed25519");
    await expect(dialog.getByText("Everyone in platform will see this host")).toBeVisible();
    await expect(dialog.getByText("bssh teams share web-1 --team platform")).toBeVisible();

    await dialog.getByRole("button", { name: "Share with platform" }).click();
    await expect.poll(async () => app.ipcCalls("teams_share")).toEqual([{ connection: "web-1", team: "t-1" }]);
    await expect(dialog).toBeHidden();
    await expect(page.getByRole("status").filter({ hasText: "Shared 'web-1' with platform" })).toBeVisible();
  });

  test("a team copy offers Update in <team>", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "More actions for db-prod@platform" }).click();
    await page.getByRole("menuitem", { name: "Update in platform" }).click();

    const dialog = page.getByRole("dialog", { name: "Update db-prod@platform in platform" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole("combobox")).toHaveCount(0);
    await dialog.getByRole("button", { name: "Update in platform" }).click();
    await expect
      .poll(async () => app.ipcCalls("teams_share"))
      .toEqual([{ connection: "db-prod@platform", team: "t-1" }]);
  });

  test("at 1024 px the tags cell shows one tag and +N, and nothing overflows", async ({ app, page }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await app.gotoApp();
    const web = row(page, "web-1");
    await expect(web.locator(".tag", { hasText: "prod" })).toBeVisible();
    const more = web.getByRole("button", { name: "3 more tags: web, eu, blue" });
    await expect(more).toHaveText("+3");
    await more.click();
    await expect(page.getByRole("dialog", { name: "Tags" })).toContainText("blue");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog", { name: "Tags" })).toHaveCount(0);

    const overflow = await page
      .locator(".view-body")
      .evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });
});
