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

test.describe("hosts row actions", () => {
  test.use({ permissions: ["clipboard-read", "clipboard-write"] });

  test("copy SSH command puts it on the clipboard and toasts", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "Copy SSH command for prod-api-01" }).click();

    await expect(
      page.getByRole("status").filter({ hasText: "SSH command copied to clipboard" }),
    ).toBeVisible();
    const clip = await page.evaluate(() => navigator.clipboard.readText());
    expect(clip).toContain("deploy@10.0.4.12");
    expect(clip).toContain("-J jump@bastion.corp");
  });

  test("duplicate creates a copy and opens the edit modal", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "Duplicate prod-api-01" }).click();

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
    await page.getByRole("button", { name: "Delete prod-api-01" }).click();

    const dialog = page.getByRole("dialog").filter({ hasText: "Delete prod-api-01?" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Delete", exact: true }).click();

    await expect.poll(async () => app.ipcCalls("remove_connection")).toEqual([{ idOrName: "id-0" }]);
    await expect(page.locator("tbody tr")).toHaveCount(HOSTS.length - 1);
    await expect(row(page, "prod-api-01")).toHaveCount(0);
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
