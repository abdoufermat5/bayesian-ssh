import { test, expect } from "./fixtures/app";

test.describe("terminals — empty state", () => {
  test("lists recent hosts and clicking one spawns a pty", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Terminals");

    await expect(page.locator(".workspace h1")).toHaveText("Terminals");
    const emptyTitle = page.getByRole("paragraph").filter({ hasText: "No open sessions" });
    await expect(emptyTitle).toBeVisible();
    await expect(page.getByText("Recent hosts")).toBeVisible();

    // Most recently used host first (mock: prod-api-01 was used 12 min ago).
    const recent = page.locator('button[title^="Connect to "]');
    await expect(recent.first()).toHaveAttribute("title", "Connect to prod-api-01");
    await expect(recent.filter({ hasText: "gpu-trainer" })).toBeVisible();

    await page.getByTitle("Connect to prod-api-01").click();

    await expect
      .poll(async () => app.ipcCalls("spawn_pty"))
      .toEqual([{ sessionId: expect.any(String), connectionName: "prod-api-01" }]);

    // The empty state is replaced by a tab for the new session.
    const tabs = page.locator('[role="tab"]');
    await expect(tabs).toHaveCount(1);
    await expect(tabs.first()).toHaveAttribute("aria-selected", "true");
    await expect(tabs.first()).toContainText("prod-api-01");
    await expect(emptyTitle).toBeHidden();
  });
});

test.describe("terminals — sessions in the tab strip", () => {
  test("connect from Hosts, open a second session, switch tabs and close one", async ({
    app,
    page,
  }) => {
    await app.gotoApp();
    await app.nav("Hosts");

    // Select a host row, then connect with Enter (the documented shortcut).
    await page.locator("tbody tr", { hasText: "staging-web" }).click();
    await page.keyboard.press("Enter");

    await expect
      .poll(async () => app.ipcCalls("spawn_pty"))
      .toContainEqual({ sessionId: expect.any(String), connectionName: "staging-web" });

    const tabs = page.locator('[role="tab"]');
    await expect(tabs).toHaveCount(1);
    await expect(tabs.first()).toContainText("staging-web");
    await expect(tabs.first()).toHaveAttribute("aria-selected", "true");

    // Second session through the "New session" launcher.
    await page.getByRole("button", { name: "New session" }).click();
    await page.getByRole("option", { name: /prod-api-02/ }).click();

    await expect
      .poll(async () => app.ipcCalls("spawn_pty"))
      .toContainEqual({ sessionId: expect.any(String), connectionName: "prod-api-02" });

    await expect(tabs).toHaveCount(2);
    const tabIds = await tabs.evaluateAll((els) => els.map((el) => el.getAttribute("data-tab-id") ?? ""));
    const [stagingId, prodId] = tabIds;
    expect(stagingId).not.toBe(prodId);

    // The newly connected session becomes active.
    await expect(page.locator(`[data-tab-id="${prodId}"]`)).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(`[data-tab-id="${stagingId}"]`)).toHaveAttribute("aria-selected", "false");
    await expect(page.getByRole("tabpanel", { name: "prod-api-02" })).toBeVisible();
    await expect(page.getByRole("tabpanel", { name: "staging-web" })).toBeHidden();

    // Switching tabs toggles selection and the visible surface.
    await page.locator(`[data-tab-id="${stagingId}"]`).click();
    await expect(page.locator(`[data-tab-id="${stagingId}"]`)).toHaveAttribute("aria-selected", "true");
    await expect(page.locator(`[data-tab-id="${prodId}"]`)).toHaveAttribute("aria-selected", "false");
    await expect(page.getByRole("tabpanel", { name: "staging-web" })).toBeVisible();
    await expect(page.getByRole("tabpanel", { name: "prod-api-02" })).toBeHidden();

    // Closing a tab terminates exactly that session.
    await page.getByRole("button", { name: "Close prod-api-02" }).click();
    await expect.poll(async () => app.ipcCalls("close_pty")).toEqual([{ sessionId: prodId }]);
    await expect(tabs).toHaveCount(1);
    await expect(tabs.first()).toContainText("staging-web");
    await expect(tabs.first()).toHaveAttribute("aria-selected", "true");
  });

  test.describe("toolbar", () => {
    // `count_active_sessions` drives the "Close all sessions" menu entry; the
    // default mock reports 0 so the entry is hidden.
    test.use({ mockOptions: { overrides: { count_active_sessions: () => 1 } } });

    test("overflow menu, font-size stepper and find bar behave", async ({ app, page }) => {
      await app.gotoApp();
      await app.nav("Terminals");
      await page.getByTitle("Connect to prod-api-01").click();
      await expect(page.locator('[role="tab"]')).toHaveCount(1);

      // --- Font-size stepper ------------------------------------------------
      const fontGroup = page.getByRole("group", { name: "Font size" });
      const size = fontGroup.locator("span[title='Font size']");
      await expect(size).toHaveText("13");

      await page.getByRole("button", { name: "Increase font size" }).click();
      await expect(size).toHaveText("14");
      await page.getByRole("button", { name: "Decrease font size" }).click();
      await page.getByRole("button", { name: "Decrease font size" }).click();
      await expect(size).toHaveText("12");

      // --- Find bar ---------------------------------------------------------
      const findToggle = page.getByRole("button", { name: "Find in terminal" });
      await expect(findToggle).toHaveAttribute("aria-pressed", "false");

      await findToggle.click();
      await expect(findToggle).toHaveAttribute("aria-pressed", "true");
      const findInput = page.getByRole("textbox", { name: "Find in scrollback" });
      await expect(findInput).toBeVisible();
      await expect(findInput).toBeFocused();
      await expect(page.getByRole("button", { name: "Next match" })).toBeVisible();
      await expect(page.getByRole("button", { name: "Previous match" })).toBeVisible();
      await findInput.fill("nvidia");

      // Escape inside the find input dismisses the bar.
      await page.keyboard.press("Escape");
      await expect(findInput).toBeHidden();
      await expect(findToggle).toHaveAttribute("aria-pressed", "false");

      // --- Overflow menu ----------------------------------------------------
      const more = page.getByRole("button", { name: "More session actions" });
      await expect(more).toHaveAttribute("aria-expanded", "false");
      await more.click();
      await expect(more).toHaveAttribute("aria-expanded", "true");

      const menu = page.getByRole("menu");
      await expect(menu.getByRole("menuitem", { name: "Open in new window" })).toBeVisible();
      await expect(menu.getByRole("menuitem", { name: "Run in background" })).toBeVisible();
      await expect(menu.getByRole("menuitem", { name: "Export scrollback" })).toBeVisible();
      await expect(menu.getByRole("menuitem", { name: "Clear screen" })).toBeVisible();
      await expect(menu.getByRole("menuitem", { name: "Manage sessions…" })).toBeVisible();
      await expect(menu.getByRole("menuitem", { name: "Close all sessions" })).toBeVisible();

      // Escape closes the menu without acting.
      await page.keyboard.press("Escape");
      await expect(menu).toBeHidden();
      await expect(more).toHaveAttribute("aria-expanded", "false");

      // "Manage sessions…" opens the session manager.
      await more.click();
      await menu.getByRole("menuitem", { name: "Manage sessions…" }).click();
      const dialog = page.getByRole("dialog", { name: "Running sessions" });
      await expect(dialog).toBeVisible();
      await expect(dialog.getByText("Nothing running elsewhere")).toBeVisible();
      await dialog.getByRole("button", { name: "Done" }).click();
      await expect(dialog).toBeHidden();

      // "Close all sessions" closes every pty and returns to the empty state.
      await more.click();
      await menu.getByRole("menuitem", { name: "Close all sessions" }).click();
      await expect.poll(async () => app.ipcCalls("close_all_ptys")).toHaveLength(1);
      await expect(page.locator('[role="tab"]')).toHaveCount(0);
      await expect(
        page.getByRole("paragraph").filter({ hasText: "No open sessions" }),
      ).toBeVisible();
    });
  });
});

test.describe("terminals — detached sessions", () => {
  test.use({ mockOptions: { detached: [{ session_id: "det-1", connection_name: "gpu-trainer" }] } });

  test("sidebar 'Background sessions' lists the session and reattach restores it as a tab", async ({
    app,
    page,
  }) => {
    await app.gotoApp();

    const background = page.locator('.sidebar [aria-label="Background sessions"]');
    await expect(background).toBeVisible();
    await background.click();

    const dialog = page.getByRole("dialog", { name: "Running sessions" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Background", { exact: true })).toBeVisible();
    await expect(dialog.getByText("gpu-trainer")).toBeVisible();

    await dialog.getByRole("button", { name: "Reattach" }).click();

    await expect.poll(async () => app.ipcCalls("reattach_pty")).toEqual([{ sessionId: "det-1" }]);
    // A successful reattach closes the manager (the session is now a tab).
    await expect(dialog).toBeHidden();

    await app.nav("Terminals");
    const tabs = page.locator('[role="tab"]');
    await expect(tabs).toHaveCount(1);
    await expect(tabs.first()).toContainText("gpu-trainer");
    await expect(tabs.first()).toHaveAttribute("aria-selected", "true");
    // The restored session is no longer listed as running elsewhere.
    await expect(page.locator('.sidebar [aria-label="Background sessions"]')).toBeHidden();
  });

  test("terminating a detached session calls close_pty and clears the sidebar", async ({
    app,
    page,
  }) => {
    await app.gotoApp();

    await page.locator('.sidebar [aria-label="Background sessions"]').click();
    const dialog = page.getByRole("dialog", { name: "Running sessions" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("gpu-trainer")).toBeVisible();

    await dialog.getByRole("button", { name: "Terminate gpu-trainer" }).click();

    await expect.poll(async () => app.ipcCalls("close_pty")).toEqual([{ sessionId: "det-1" }]);
    await expect(dialog.getByText("Nothing running elsewhere")).toBeVisible();
    // No sessions left elsewhere => the sidebar control disappears.
    await expect(page.locator('.sidebar [aria-label="Background sessions"]')).toBeHidden();
  });
});

test.describe("terminals — connect progress and failure", () => {
  async function connect(app: any, page: any, host: string) {
    await page.getByTitle(`Connect to ${host}`).click();
    await expect.poll(async () => (await app.ipcCalls("spawn_pty")).length).toBeGreaterThan(0);
    const calls = (await app.ipcCalls("spawn_pty")) as Array<{ sessionId: string }>;
    return calls[calls.length - 1].sessionId;
  }

  test("shows Connecting until ssh prints something, and Esc cancels", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Terminals");
    const id = await connect(app, page, "prod-api-01");

    const status = page.getByRole("status").filter({ hasText: /Connecting to .*prod-api-01|Connecting to .*@/ });
    await expect(status).toBeVisible();
    await page.keyboard.press("Escape");
    await expect.poll(async () => app.ipcCalls("close_pty")).toEqual([{ sessionId: id }]);
    await expect(page.locator('[role="tab"]')).toHaveCount(0);
  });

  test("first output ends the connecting state", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Terminals");
    const id = await connect(app, page, "prod-api-01");
    await page.evaluate((sid) => (window as any).__MOCK_EMIT("pty-output", { session_id: sid, data: "Last login\r\n" }), id);
    await expect(page.getByRole("status").filter({ hasText: "Connecting to" })).toBeHidden();
  });

  test("an ssh failure stays in the tab with Retry and Edit host", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Terminals");
    const id = await connect(app, page, "prod-api-01");
    await page.evaluate((sid) => {
      const emit = (window as any).__MOCK_EMIT;
      emit("pty-output", { session_id: sid, data: "ssh: connect to host x port 22: Connection refused\r\n" });
      emit("pty-exit-status", { session_id: sid, exit_code: 255 });
      emit("pty-exit", sid);
    }, id);

    const alert = page.getByRole("alert").filter({ hasText: "refused the connection" });
    await expect(alert).toBeVisible();
    await expect(alert.getByRole("button", { name: "Edit host" })).toBeVisible();
    await alert.getByRole("button", { name: "Retry" }).click();
    await expect.poll(async () => (await app.ipcCalls("spawn_pty")).length).toBe(2);
    await expect(alert).toBeHidden();
  });
});

test.describe("terminals — tab shortcuts", () => {
  test("Ctrl+Tab cycles tabs, Ctrl+Shift+W closes, Ctrl+Shift+T reopens", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Terminals");
    await page.getByTitle("Connect to prod-api-01").click();
    await page.getByRole("button", { name: "New session" }).click();
    await page.getByRole("option", { name: /prod-api-02/ }).click();
    const tabs = page.locator('[role="tab"]');
    await expect(tabs).toHaveCount(2);
    await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");

    await page.keyboard.press("Control+Tab");
    await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("Control+Shift+Tab");
    await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");

    await page.keyboard.press("Control+Shift+W");
    await expect(tabs).toHaveCount(1);
    await page.keyboard.press("Control+Shift+T");
    await expect(tabs).toHaveCount(2);
    await expect(tabs.nth(1)).toContainText("prod-api-02");
  });
});

test.describe("terminals — New session launcher", () => {
  test("lists every host even when the Hosts view is filtered", async ({ app, page }) => {
    await app.gotoApp();
    await page.locator(".search-input").fill("api");
    await expect(page.locator("tbody tr", { hasText: "gpu-trainer" })).toHaveCount(0);

    await page.locator("tbody tr", { hasText: "prod-api-01" }).click();
    await page.keyboard.press("Enter");
    await expect(page.locator('[role="tab"]')).toHaveCount(1);
    await page.getByRole("button", { name: "New session" }).click();
    await expect(page.getByRole("option", { name: /gpu-trainer/ })).toBeVisible();
  });
});
