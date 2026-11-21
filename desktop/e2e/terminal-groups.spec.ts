import { test, expect, type Page } from "./fixtures/app";

/** Session tabs in the Console strip; the pinned "Hosts" tab carries no data-tab-id. */
const sessionTabs = (page: Page) => page.locator('[role="tab"][data-tab-id]');

/** Open sessions to `hosts` in order: the first from the host list, the rest through the launcher. */
async function openSessions(page: Page, hosts: string[]): Promise<string[]> {
  await page.getByRole("button", { name: `Connect to ${hosts[0]}`, exact: true }).click();
  for (const host of hosts.slice(1)) {
    await page.getByRole("button", { name: "New session" }).click();
    await page.getByRole("option", { name: new RegExp(host) }).click();
  }
  const tabs = sessionTabs(page);
  await expect(tabs).toHaveCount(hosts.length);
  const ids: string[] = [];
  for (const host of hosts) {
    ids.push((await tabs.filter({ hasText: host }).getAttribute("data-tab-id")) ?? "");
  }
  return ids;
}

function tabOrder(page: Page): Promise<string[]> {
  return sessionTabs(page).evaluateAll((els) => els.map((el) => el.textContent?.trim() ?? ""));
}

test.describe("terminals — tab groups", () => {
  test("group tabs, collapse a group and reopen its hosts after a reload", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Console");
    const [prod1, , prod2] = await openSessions(page, ["prod-api-01", "staging-web", "prod-api-02"]);

    // New group from the tab menu; its editor opens to name it.
    await page.locator(`[data-tab-id="${prod1}"]`).click({ button: "right" });
    await page.getByRole("menuitem", { name: "Add to new group" }).click();
    const editor = page.getByRole("dialog", { name: "Edit group" });
    await editor.getByLabel("Group name").fill("prod");
    await editor.getByRole("radio", { name: "red" }).click();
    await expect(editor.getByRole("radio", { name: "red" })).toHaveAttribute("aria-checked", "true");
    await editor.getByLabel("Group name").press("Enter");
    await expect(editor).toBeHidden();

    const chip = page.getByRole("button", { name: /^prod, 1 session$/ });
    await expect(chip).toBeVisible();

    // A tab added to the group moves next to its other tabs.
    await page.locator(`[data-tab-id="${prod2}"]`).click({ button: "right" });
    await page.getByRole("menuitem", { name: "Add to prod" }).click();
    await expect(page.getByRole("button", { name: /^prod, 2 sessions$/ })).toBeVisible();
    await expect.poll(() => tabOrder(page)).toEqual([
      expect.stringContaining("prod-api-01"),
      expect.stringContaining("prod-api-02"),
      expect.stringContaining("staging-web"),
    ]);

    // Collapsing the active tab's group hides its tabs and moves to one in view.
    await page.locator(`[data-tab-id="${prod2}"]`).click();
    await page.getByRole("button", { name: /^prod, 2 sessions$/ }).click();
    await expect(page.getByRole("button", { name: /^prod, 2 sessions$/ })).toHaveAttribute("aria-expanded", "false");
    await expect(sessionTabs(page)).toHaveCount(1);
    await expect(sessionTabs(page).first()).toContainText("staging-web");
    await expect(sessionTabs(page).first()).toHaveAttribute("aria-selected", "true");

    // Ctrl+Tab skips the collapsed tabs.
    await page.keyboard.press("Control+Tab");
    await expect(sessionTabs(page).first()).toHaveAttribute("aria-selected", "true");

    // The group and its hosts survive a restart: the launcher offers to reopen it.
    await page.reload();
    await expect(page.locator(".app-shell")).toBeVisible({ timeout: 15000 });
    await app.nav("Console");
    await page.getByRole("button", { name: "Connect to staging-web", exact: true }).click();
    await page.getByRole("button", { name: "New session" }).click();
    await page.getByRole("button", { name: /Open prod/ }).click();
    await expect(page.getByRole("button", { name: /^prod, 2 sessions$/ })).toBeVisible();
    await expect.poll(() => tabOrder(page)).toEqual([
      expect.stringContaining("staging-web"),
      expect.stringContaining("prod-api-01"),
      expect.stringContaining("prod-api-02"),
    ]);

    // Ungroup forgets it; the sessions stay open.
    await page.getByRole("button", { name: /^prod, 2 sessions$/ }).click({ button: "right" });
    await page.getByRole("dialog", { name: "Edit group" }).getByRole("button", { name: "Ungroup" }).click();
    await expect(page.locator("[data-group-id]")).toHaveCount(0);
    await expect(sessionTabs(page)).toHaveCount(3);
    expect(await page.evaluate(() => localStorage.getItem("bayesian-ssh-tab-groups"))).toBe("[]");
  });

  test("dragging a tab reorders it, and into a group's middle joins the group", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Console");
    const [a, b, c] = await openSessions(page, ["prod-api-01", "staging-web", "prod-api-02"]);

    const box = async (id: string) => (await page.locator(`[data-tab-id="${id}"]`).boundingBox())!;
    const dragTo = async (id: string, x: number, y: number) => {
      const from = await box(id);
      await page.mouse.move(from.x + 10, from.y + from.height / 2);
      await page.mouse.down();
      await page.mouse.move(from.x + 30, from.y + from.height / 2, { steps: 4 });
      await page.mouse.move(x, y, { steps: 8 });
      await page.mouse.up();
    };

    // Drop c before a.
    const first = await box(a);
    await dragTo(c, first.x + 4, first.y + first.height / 2);
    await expect.poll(() => tabOrder(page)).toEqual([
      expect.stringContaining("prod-api-02"),
      expect.stringContaining("prod-api-01"),
      expect.stringContaining("staging-web"),
    ]);

    // Group c and a, then drop b between them: it joins the group.
    await page.locator(`[data-tab-id="${c}"]`).click({ button: "right" });
    await page.getByRole("menuitem", { name: "Add to new group" }).click();
    await page.getByRole("dialog", { name: "Edit group" }).getByLabel("Group name").press("Escape");
    await page.locator(`[data-tab-id="${a}"]`).click({ button: "right" });
    await page.getByRole("menuitem", { name: "Add to Unnamed group" }).click();
    await expect(page.getByRole("button", { name: /^Unnamed group, 2 sessions$/ })).toBeVisible();

    const target = await box(a);
    await dragTo(b, target.x + 4, target.y + target.height / 2);
    await expect(page.getByRole("button", { name: /^Unnamed group, 3 sessions$/ })).toBeVisible();
    await expect.poll(() => tabOrder(page)).toEqual([
      expect.stringContaining("prod-api-02"),
      expect.stringContaining("staging-web"),
      expect.stringContaining("prod-api-01"),
    ]);
    // A drag is not a click on the tab, and nothing popped out.
    expect(await app.ipcCalls("open_terminal_window")).toEqual([]);
  });
});

test.describe("terminals — split screen", () => {
  test("open a session beside the current one, resize the split and take a pane out", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Console");
    await openSessions(page, ["prod-api-01"]);

    await page.getByRole("button", { name: "Split right" }).click();
    await expect(page.getByText("Opens right of prod-api-01")).toBeVisible();
    await page.getByRole("option", { name: /staging-web/ }).click();

    const left = page.getByRole("tabpanel", { name: "prod-api-01" });
    const right = page.getByRole("tabpanel", { name: "staging-web" });
    await expect(left).toBeVisible();
    await expect(right).toBeVisible();
    const leftBox = (await left.boundingBox())!;
    const rightBox = (await right.boundingBox())!;
    expect(rightBox.x).toBeGreaterThan(leftBox.x + leftBox.width - 2);
    expect(Math.abs(leftBox.width - rightBox.width)).toBeLessThan(3);

    // The new pane is focused; clicking the other pane focuses its tab.
    await expect(sessionTabs(page).filter({ hasText: "staging-web" })).toHaveAttribute("aria-selected", "true");
    await left.click();
    await expect(sessionTabs(page).filter({ hasText: "prod-api-01" })).toHaveAttribute("aria-selected", "true");
    await expect(right).toBeVisible();

    // Keyboard resize.
    const divider = page.getByRole("separator", { name: "Resize split" });
    await divider.focus();
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("ArrowRight");
    await expect(divider).toHaveAttribute("aria-valuenow", "60");
    expect((await left.boundingBox())!.width).toBeGreaterThan(leftBox.width + 20);

    // Pointer resize.
    const handle = (await divider.boundingBox())!;
    const area = (await page.locator("[data-terminals-view]").boundingBox())!;
    await page.mouse.move(handle.x + handle.width / 2, handle.y + handle.height / 2);
    await page.mouse.down();
    await page.mouse.move(area.x + area.width * 0.3, handle.y + handle.height / 2, { steps: 6 });
    await page.mouse.up();
    await expect.poll(async () => Number(await divider.getAttribute("aria-valuenow"))).toBeLessThan(40);

    // Taking a pane out keeps its session and shows it alone.
    await page.getByRole("button", { name: "Remove staging-web from split" }).click();
    await expect(page.getByRole("separator", { name: "Resize split" })).toHaveCount(0);
    await expect(right).toBeVisible();
    await expect(left).toBeHidden();
    await expect(sessionTabs(page)).toHaveCount(2);
    expect(await app.ipcCalls("close_pty")).toEqual([]);
  });

  test("dragging a tab onto the bottom edge of the terminal splits it down", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Console");
    const [, staging] = await openSessions(page, ["prod-api-01", "staging-web"]);

    // Show prod-api-01, then drag staging-web onto its bottom edge.
    await sessionTabs(page).filter({ hasText: "prod-api-01" }).click();
    const from = (await page.locator(`[data-tab-id="${staging}"]`).boundingBox())!;
    const panel = (await page.getByRole("tabpanel", { name: "prod-api-01" }).boundingBox())!;
    await page.mouse.move(from.x + 10, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(panel.x + panel.width / 2, panel.y + panel.height - 20, { steps: 10 });
    await expect(page.getByText("Split bottom")).toBeVisible();
    await page.mouse.up();

    const top = (await page.getByRole("tabpanel", { name: "prod-api-01" }).boundingBox())!;
    const bottom = (await page.getByRole("tabpanel", { name: "staging-web" }).boundingBox())!;
    expect(bottom.y).toBeGreaterThan(top.y + top.height - 2);
    await expect(page.getByRole("separator", { name: "Resize split" })).toHaveAttribute("aria-orientation", "horizontal");

    // Closing a pane's session dissolves the split.
    await page.getByRole("button", { name: "Close staging-web" }).click();
    await expect(page.getByRole("separator", { name: "Resize split" })).toHaveCount(0);
    await expect(page.getByRole("tabpanel", { name: "prod-api-01" })).toBeVisible();
  });

  test("dragging a tab to the middle of the terminal opens it in a window", async ({ app, page }) => {
    await app.gotoApp();
    await app.nav("Console");
    const [prod] = await openSessions(page, ["prod-api-01"]);

    const from = (await page.locator(`[data-tab-id="${prod}"]`).boundingBox())!;
    const panel = (await page.getByRole("tabpanel", { name: "prod-api-01" }).boundingBox())!;
    await page.mouse.move(from.x + 10, from.y + from.height / 2);
    await page.mouse.down();
    await page.mouse.move(panel.x + panel.width / 2, panel.y + panel.height / 2, { steps: 10 });
    await expect(page.getByText("Open in new window")).toBeVisible();
    await page.mouse.up();
    await app.waitForIpc("open_terminal_window");
  });
});
