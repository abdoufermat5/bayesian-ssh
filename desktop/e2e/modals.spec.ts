import { test, expect, type Page } from "./fixtures/app";

/** Open the sidebar profile popover and click "Manage profiles…". */
async function openProfilesModal(page: Page) {
  await page.locator('.sidebar [aria-haspopup="menu"]').click();
  await page.getByRole("menuitem", { name: "Manage profiles…" }).click();
  await expect(page.getByRole("dialog", { name: "Profiles" })).toBeVisible();
}

test.describe("profiles modal", () => {
  test("create calls create_environment with the typed name", async ({ app, page }) => {
    await app.gotoApp();
    await openProfilesModal(page);

    const dialog = page.getByRole("dialog", { name: "Profiles" });
    await dialog.locator("#e-name").fill("staging");
    await dialog.getByRole("button", { name: "Create profile" }).click();

    await expect.poll(async () => app.ipcCalls("create_environment")).toEqual([{ name: "staging" }]);
    await expect(dialog).toBeHidden();
  });

  test("delete confirm calls remove_environment", async ({ app, page }) => {
    await app.gotoApp();
    await openProfilesModal(page);

    await page.getByRole("button", { name: "Delete profile work" }).click();
    const confirm = page.getByRole("dialog", { name: "Delete work?" });
    await expect(confirm).toBeVisible();

    await confirm.getByRole("button", { name: "Delete", exact: true }).click();

    await expect.poll(async () => app.ipcCalls("remove_environment")).toEqual([{ name: "work" }]);
    await expect(confirm).toBeHidden();
    await expect(page.getByRole("button", { name: "Delete profile work" })).toHaveCount(0);
  });
});

test.describe("ssh agent modal", () => {
  test("renders loaded keys and add-key calls pick_key_file then add_key_to_agent", async ({
    app,
    page,
  }) => {
    await app.gotoApp();
    await page.locator('.sidebar [aria-label="SSH agent"]').click();

    const dialog = page.getByRole("dialog", { name: "SSH agent" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Running")).toBeVisible();
    await expect(dialog.getByText("SHA256:abc")).toBeVisible();
    await expect(dialog.getByText("me@laptop")).toBeVisible();

    await dialog.getByRole("button", { name: "Add key" }).click();

    await expect.poll(async () => app.ipcCalls("pick_key_file")).toHaveLength(1);
    await expect
      .poll(async () => app.ipcCalls("add_key_to_agent"))
      .toEqual([{ keyPath: "/home/me/.ssh/id_ed25519" }]);
  });
});

test.describe("kerberos modal", () => {
  test("valid ticket shows status and renew calls renew_kerberos_ticket", async ({ app, page }) => {
    await app.gotoApp();
    await page.locator('.sidebar [aria-label="Kerberos ticket"]').click();

    const dialog = page.getByRole("dialog", { name: "Kerberos ticket" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Valid", { exact: true })).toBeVisible();
    await expect(dialog.getByText("me@CORP.EXAMPLE")).toBeVisible();

    await dialog.getByRole("button", { name: "Renew ticket" }).click();

    await expect.poll(async () => app.ipcCalls("renew_kerberos_ticket")).toHaveLength(1);
    await expect(dialog).toBeHidden();
  });
});

test.describe("kerberos indicator (no Kerberos connection)", () => {
  test.use({
    mockOptions: {
      overrides: {
        get_connections: () => [
          {
            id: "id-plain",
            name: "plain-host",
            host: "plain.example.com",
            user: "deploy",
            port: 22,
            tags: [],
            use_kerberos: false,
            created_at: "2026-01-01T00:00:00Z",
          },
        ],
      },
    },
  });

  test("stays hidden even with Kerberos tools and a valid ticket", async ({ app, page }) => {
    await app.gotoApp();
    await expect(page.locator("tr", { hasText: "plain-host" })).toBeVisible();
    await expect(page.locator('.sidebar [aria-label="Kerberos ticket"]')).toHaveCount(0);
  });
});

test.describe("kerberos modal (no ticket)", () => {
  test.use({ mockOptions: { kerberos: "none" } });

  test("missing ticket shows the acquire flow and calls acquire_kerberos_ticket", async ({
    app,
    page,
  }) => {
    await app.gotoApp();
    await page.locator('.sidebar [aria-label="Kerberos ticket"]').click();

    const dialog = page.getByRole("dialog", { name: "Kerberos ticket" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("No ticket", { exact: true })).toBeVisible();
    await expect(dialog.getByRole("heading", { name: "Get a ticket" })).toBeVisible();

    const submit = dialog.getByRole("button", { name: "Get ticket" });
    await expect(submit).toBeDisabled();

    await dialog.locator("#kerberos-password").fill("hunter2");
    await expect(submit).toBeEnabled();
    await submit.click();

    await expect.poll(async () => app.ipcCalls("acquire_kerberos_ticket")).toHaveLength(1);
    const calls = await app.ipcCalls("acquire_kerberos_ticket");
    expect(calls[0]).toMatchObject({
      password: "hunter2",
      forwardable: true,
      proxiable: false,
    });
  });
});

test.describe("kerberos modal (tools missing)", () => {
  test.use({ mockOptions: { kerberos: "notools" } });

  test("shows the tools-unavailable alert without an acquire affordance", async ({ app, page }) => {
    await app.gotoApp();
    // With Kerberos tools missing the sidebar hides its Kerberos entry, so reach
    // the dialog the way a user would: connecting to a Kerberos host surfaces it.
    const row = page.locator("tr", { hasText: "prod-db-primary" });
    await row.getByRole("button", { name: "Connect" }).click();

    const dialog = page.getByRole("dialog", { name: "Kerberos ticket" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Kerberos tools not found")).toBeVisible();
    await expect(dialog.getByRole("button", { name: "Get ticket" })).toHaveCount(0);
    await expect(dialog.getByRole("button", { name: "Renew ticket" })).toHaveCount(0);
  });
});

test.describe("delete confirmation", () => {
  test("host delete: Cancel aborts, Confirm removes the host", async ({ app, page }) => {
    await app.gotoApp();
    const dialog = page.getByRole("dialog", { name: "Delete prod-api-01?" });

    await page.getByRole("button", { name: "Delete prod-api-01" }).click();
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("deploy@10.0.4.12:22")).toBeVisible();

    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toBeHidden();
    expect(await app.ipcCalls("remove_connection")).toEqual([]);

    await page.getByRole("button", { name: "Delete prod-api-01" }).click();
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Delete", exact: true }).click();

    await expect
      .poll(async () => app.ipcCalls("remove_connection"))
      .toEqual([{ idOrName: "id-0" }]);
    await expect(dialog).toBeHidden();
    await expect(page.getByRole("button", { name: "Delete prod-api-01" })).toHaveCount(0);
  });
});

test.describe("shortcuts modal", () => {
  test("lists the known shortcuts", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "Keyboard shortcuts" }).click();

    const dialog = page.getByRole("dialog", { name: "Keyboard shortcuts" });
    await expect(dialog).toBeVisible();

    await expect(dialog.getByText("Command palette")).toBeVisible();
    await expect(dialog.getByText("Focus filter")).toBeVisible();
    await expect(dialog.getByText("New host")).toBeVisible();

    const row = (name: string) => dialog.locator("li").filter({ hasText: name });

    await expect(row("Command palette").locator("kbd")).toHaveText(["Ctrl", "K"]);
    await expect(row("Focus filter").locator("kbd")).toHaveText(["/"]);
    await expect(row("New host").locator("kbd")).toHaveText(["N", "Ctrl", "N"]);

    const views: Array<[string, string]> = [
      ["Hosts", "1"],
      ["Terminals", "2"],
      ["Keys", "3"],
      ["Security audit", "4"],
      ["History", "5"],
      ["Settings", "6"],
    ];
    for (const [name, key] of views) {
      await expect(row(name).locator("kbd")).toHaveText([key]);
    }
  });
});

test.describe("about modal", () => {
  test("shows the app version from get_app_version", async ({ app, page }) => {
    await app.gotoApp();
    await page.getByRole("button", { name: "About Bayesian SSH" }).click();

    const dialog = page.getByRole("dialog", { name: "About Bayesian SSH" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Version 2.4.0")).toBeVisible();
  });
});
